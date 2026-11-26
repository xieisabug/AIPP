pub mod apply;

pub mod preset;
pub mod setup;

use crate::db::{assistant_db::AssistantDatabase, llm_db::LLMDatabase, system_db::SystemDatabase};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

pub(super) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
static SESSION: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tauri::command]
pub async fn check_onboarding_agent(
    app: AppHandle,
    window: tauri::WebviewWindow,
    provider_id: i64,
    model: Option<String>,
) -> Result<Vec<String>, String> {
    require_window(&window)?;
    let llm = LLMDatabase::new(&app).map_err(err)?;
    let provider = llm.get_llm_provider(provider_id).map_err(err)?;
    let settings = llm.get_llm_provider_config(provider_id).map_err(err)?;
    let features = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let channel = crate::api::ai::light_model::LocalChannel::new(&app, &provider, &settings, &features).map_err(err)?;
    let catalog = tokio::time::timeout(std::time::Duration::from_secs(75), channel.catalog(&app, provider_id)).await
        .map_err(|_| format!("{} 获取模型列表超过 75 秒", provider.name))?
        .map_err(|e| format!("{} 获取模型列表失败：{e}", provider.name))?;
    if catalog.is_empty() {
        return Err(format!("{} 没有返回可用模型，请检查登录和配置", provider.name));
    }
    if let Some(code) = &model {
        if !catalog.iter().any(|(found, _)| found == code) {
            return Err(format!("{} 当前未提供模型 {code}", provider.name));
        }
    }
    let tx = llm.conn.unchecked_transaction().map_err(err)?;
    if llm.get_llm_provider(provider_id).map_err(err)?.api_type != provider.api_type ||
        llm.get_llm_provider_config(provider_id).map_err(err)? != settings {
        return Err(format!("验证期间 {} 的配置已变更，请重试", provider.name));
    }
    for (code, name) in &catalog {
        tx.execute("INSERT INTO llm_model(llm_provider_id,code,name,description) SELECT ?1,?2,?3,'' WHERE NOT EXISTS
            (SELECT 1 FROM llm_model WHERE llm_provider_id=?1 AND code=?2)", params![provider_id, code, name]).map_err(err)?;
    }
    tx.commit().map_err(err)?;
    Ok(catalog.into_iter().map(|(code, _)| code).collect())
}

/// Only the manual test button runs a generation. Discovery never calls this command.
#[tauri::command]
pub async fn test_onboarding_agent_model(
    app: AppHandle,
    window: tauri::WebviewWindow,
    provider_id: i64,
    model_code: String,
) -> Result<(), String> {
    require_window(&window)?;
    let db = LLMDatabase::new(&app).map_err(err)?;
    let provider = db.get_llm_provider(provider_id).map_err(err)?;
    let model = match model_code.as_str() {
        "acp" | "__agent_default__" => None,
        code => {
            db.get_llm_model_detail(&provider_id, code)
                .map_err(|e| format!("无法读取 {} 的模型 {code}：{e}", provider.name))?;
            Some(code)
        }
    };
    let settings = db.get_llm_provider_config(provider_id).map_err(err)?;
    let features = app.state::<crate::FeatureConfigState>().config_feature_map.lock().await.clone();
    let channel = crate::api::ai::light_model::LocalChannel::new(&app, &provider, &settings, &features).map_err(err)?;
    channel.text(model, "请仅回复 OK。").await
        .map(|_| ())
        .map_err(|e| format!("{}（{}）测试失败：{e}", provider.name, model.unwrap_or("通道默认模型")))
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    #[serde(default)]
    providers: Vec<DraftProvider>,
    recharge_provider_id: Option<i64>,
    config: Option<apply::Config>,
    // Previous result while the user changes a choice on the completion page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    previous_config: Option<apply::Config>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pending_role_change: Option<apply::PendingRoleChange>,
    // Planned seed replacements survive process exits between the LLM and system DB writes.
    #[serde(default)]
    seed_targets: std::collections::BTreeMap<i64, i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DraftProvider {
    provider_id: i64,
    owned: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    state: Option<String>,
    path: Option<String>,
    step: Option<String>,
    can_open: bool,
    rerun: bool,
    debug: bool,
    workspace_path: String,
    workspace_locked: bool,
    active_light: Option<String>,
    active_review: Option<String>,
    draft: Draft,
}

fn db(app: &AppHandle) -> Result<SystemDatabase, String> {
    SystemDatabase::new(app).map_err(err)
}
fn local(db: &SystemDatabase, key: &str) -> Result<Option<String>, String> {
    Ok(Some(db.get_config(key).map_err(err)?).filter(|s| !s.is_empty()))
}
fn experiment(db: &SystemDatabase, key: &str) -> Result<String, String> {
    Ok(db
        .get_feature_config("experimental", key)
        .map_err(err)?
        .map(|v| v.value)
        .unwrap_or_default())
}
fn butler_configured(db: &SystemDatabase) -> Result<bool, String> {
    Ok(!experiment(db, "butler_model_id")?.trim().is_empty()
        && !experiment(db, "butler_main_workspace_path")?.trim().is_empty())
}

fn has_manual_configuration(llm: &LLMDatabase, assistants: &AssistantDatabase) -> Result<bool, String> {
    // Older installations can contain empty, disabled official providers.
    let has_provider: bool = llm.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM llm_provider p
         WHERE NOT EXISTS (SELECT 1 FROM onboarding_draft_provider d WHERE d.provider_id=p.id)
           AND (p.is_official=0 OR p.is_enabled=1
             OR EXISTS (SELECT 1 FROM llm_model m WHERE m.llm_provider_id=p.id)
             OR EXISTS (SELECT 1 FROM llm_provider_config c WHERE c.llm_provider_id=p.id AND trim(c.value)<>'')))",
        [], |r| r.get(0),
    ).map_err(err)?;
    if has_provider {
        return Ok(true);
    }
    // Assistant 1 is preinstalled with an empty model binding.
    assistants.conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM assistant a WHERE a.id<>1
         OR EXISTS (SELECT 1 FROM assistant_model m WHERE m.assistant_id=a.id AND m.provider_id>0))",
        [], |r| r.get(0),
    ).map_err(err)
}

fn can_open_manual_entry(app: &AppHandle, db: &SystemDatabase) -> Result<bool, String> {
    Ok(cfg!(desktop) && !butler_configured(db)? && !has_manual_configuration(
        &LLMDatabase::new(app).map_err(err)?,
        &AssistantDatabase::new(app).map_err(err)?,
    )?)
}

fn put(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute("INSERT INTO system_config(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",params![key,value]).map_err(err)?;
    Ok(())
}
fn draft(db: &SystemDatabase) -> Result<Draft, String> {
    local(db, "onboarding_draft")?
        .map(|v| serde_json::from_str(&v).map_err(|e| format!("读取首次引导草稿失败：{e}")))
        .unwrap_or_else(|| Ok(Draft::default()))
}
fn save_draft(conn: &Connection, d: &Draft) -> Result<(), String> {
    put(conn, "onboarding_draft", &serde_json::to_string(d).map_err(err)?)
}
fn complete(conn: &Connection) -> Result<(), String> {
    put(conn, "onboarding_state", "completed")?;
    put(conn, "onboarding_completed_at", &chrono::Utc::now().to_rfc3339())
}
fn default_workspace(app: &AppHandle) -> Result<String, String> {
    Ok(app.path().home_dir().map_err(err)?.join("AIPP").to_string_lossy().into_owned())
}

fn workspace_locked(state: Option<&str>, configured_path: &str) -> bool {
    state == Some("completed") && !configured_path.trim().is_empty()
}

#[tauri::command]
pub fn get_onboarding_state(app: AppHandle) -> Result<Status, String> {
    let db = db(&app)?;
    let state = local(&db, "onboarding_state")?;
    let workspace = experiment(&db, "butler_main_workspace_path")?;
    let step = local(&db, "onboarding_step")?;
    let summary = |key| -> Result<Option<String>, String> {
        Ok(db
            .get_feature_config("conversation_summary", key)
            .map_err(err)?
            .map(|v| v.value)
            .filter(|v| !v.trim().is_empty()))
    };
    let explicit_review = summary("auto_review_model")?
        .zip(summary("auto_review_provider_id")?)
        .map(|(code, id)| format!("{code}%%{id}"));
    let active_review =
        explicit_review.or(summary(crate::api::ai::config::DEFAULT_DECISION_MODEL_KEY)?.filter(
            |selection| crate::api::ai::config::parse_combined_model_selection(selection).is_some(),
        ));
    Ok(Status {
        active_light: summary("default_light_model")?,
        active_review,
        rerun: state.as_deref() == Some("completed") && step.as_deref() != Some("applied"),
        debug: cfg!(debug_assertions),
        workspace_locked: workspace_locked(state.as_deref(), &workspace),
        can_open: can_open_manual_entry(&app, &db)?,
        state,
        path: local(&db, "onboarding_path")?,
        step,
        workspace_path: if workspace.trim().is_empty() {
            default_workspace(&app)?
        } else {
            workspace
        },
        draft: draft(&db)?,
    })
}

enum Source {
    Usable,
    MissingKey,
    MissingAgent,
    None,
}

/// Copilot's OAuth token is saved in api_key and exchanged by copilot_token_manager.
fn has_api_credentials(
    llm: &LLMDatabase,
    provider: &crate::db::llm_db::LLMProvider,
) -> Result<bool, String> {
    if matches!(provider.api_type.as_str(), "ollama" | "magpie") {
        return Ok(true);
    }
    Ok(llm
        .get_llm_provider_config(provider.id)
        .map_err(err)?
        .iter()
        .any(|config| config.name == "api_key" && !config.value.trim().is_empty()))
}
fn sources(llm: &LLMDatabase, installed: Option<&[i64]>) -> Result<Source, String> {
    let api = llm.get_filtered_models_for_select(0)?;
    let mut missing_key = false;
    let mut checked = std::collections::HashSet::new();
    for (_, _, _, id) in api {
        if !checked.insert(id) {
            continue;
        }
        let provider = llm.get_llm_provider(id).map_err(err)?;
        if has_api_credentials(llm, &provider)? {
            return Ok(Source::Usable);
        }
        missing_key = true;
    }
    let agents = llm.get_filtered_models_for_select(4)?;
    if agents.iter().any(|m| installed.is_none_or(|ids| ids.contains(&m.3))) {
        return Ok(Source::Usable);
    }
    Ok(if missing_key {
        Source::MissingKey
    } else if !agents.is_empty() {
        Source::MissingAgent
    } else {
        Source::None
    })
}

fn should_show(db: &SystemDatabase, llm: &LLMDatabase) -> Result<bool, String> {
    let state = local(db, "onboarding_state")?;
    if local(db, "onboarding_step")?.as_deref() == Some("source_ready") {
        return Ok(true);
    }
    if butler_configured(db)? {
        if state.as_deref() != Some("completed") {
            let tx = db.conn.unchecked_transaction().map_err(err)?;
            complete(&tx)?;
            tx.commit().map_err(err)?;
        }
        return Ok(false);
    }
    match state.as_deref() {
        Some("skipped" | "completed") => Ok(false),
        Some("in_progress") => Ok(true),
        _ if matches!(sources(llm, None)?, Source::Usable) => {
            let tx = db.conn.unchecked_transaction().map_err(err)?;
            complete(&tx)?;
            tx.commit().map_err(err)?;
            Ok(false)
        }
        _ => Ok(true),
    }
}

fn require_window(window: &tauri::WebviewWindow) -> Result<(), String> {
    if cfg!(desktop) && window.label() == "onboarding" {
        Ok(())
    } else {
        Err("请在桌面首次引导窗口中执行此操作".into())
    }
}
fn is_owned(llm: &LLMDatabase, id: i64) -> Result<bool, String> {
    llm.conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM onboarding_draft_provider WHERE provider_id=?)",
            [id],
            |r| r.get(0),
        )
        .map_err(err)
}
fn release(llm: &LLMDatabase, id: i64) -> Result<(), String> {
    llm.conn
        .execute("DELETE FROM onboarding_draft_provider WHERE provider_id=?", [id])
        .map_err(err)?;
    Ok(())
}

fn notify_provider_change(app: &AppHandle) {
    let _ = app.emit("llm_provider_changed", ());
    crate::sync::schedule_sync_after_local_change(app);
}
fn notify_state_change(app: &AppHandle) {
    let _ = app.emit("onboarding-state-changed", ());
}

// Only ids explicitly created for this session are eligible for cleanup. Removing an
// already deleted draft is safe when a previous process exited before saving the checkpoint.
fn remove_owned(llm: &LLMDatabase, id: i64) -> Result<(), String> {
    let _journals =
        crate::db::cascade_delete::prepare_transaction(&llm.conn, "llm_provider").map_err(err)?;
    let tx = llm.conn.unchecked_transaction().map_err(err)?;
    if is_owned(llm, id)? {
        match llm.get_llm_provider(id) {
            Ok(provider) if !provider.is_official && !provider.is_enabled => {
                llm.delete_llm_provider(id).map_err(err)?;
            }
            Ok(_) | Err(rusqlite::Error::QueryReturnedNoRows) => {}
            Err(e) => return Err(err(e)),
        }
        release(llm, id)?;
    }
    tx.commit().map_err(err)
}
fn clear_unverified_except(
    db: &SystemDatabase,
    llm: &LLMDatabase,
    d: &mut Draft,
    keep: &[i64],
) -> Result<bool, String> {
    // This table also contains providers from a process that exited before the UI got its id.
    let mut stmt =
        llm.conn.prepare("SELECT provider_id FROM onboarding_draft_provider").map_err(err)?;
    let ids = stmt
        .query_map([], |row| row.get::<_, i64>(0))
        .map_err(err)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(err)?;
    drop(stmt);
    let mut removed_any = false;
    for id in ids {
        if !keep.contains(&id) {
            remove_owned(llm, id)?;
            removed_any = true;
        }
    }
    d.providers.retain(|p| keep.contains(&p.provider_id));
    save_draft(&db.conn, d)?;
    Ok(removed_any)
}
fn clear_unverified(
    db: &SystemDatabase,
    llm: &LLMDatabase,
    d: &mut Draft,
    keep: Option<i64>,
) -> Result<(), String> {
    let keep_slice: Vec<i64> = keep.into_iter().collect();
    clear_unverified_except(db, llm, d, &keep_slice)?;
    Ok(())
}

#[cfg(desktop)]
pub(crate) fn startup(app: &AppHandle) -> Result<bool, String> {
    let db = db(app)?;
    let llm = LLMDatabase::new(app).map_err(err)?;
    match local(&db, "onboarding_step")?.as_deref() {
        Some("path_chosen") => clear_unverified(&db, &llm, &mut draft(&db)?, None)?,
        Some("awaiting_recharge") => {
            let mut d = draft(&db)?;
            let keep = d.recharge_provider_id;
            clear_unverified(&db, &llm, &mut d, keep)?;
        }
        _ => {}
    }
    should_show(&db, &llm)
}

#[cfg(desktop)]
pub(crate) fn intercept_home(app: &AppHandle) -> bool {
    let show = (|| should_show(&db(app)?, &LLMDatabase::new(app).map_err(err)?))();
    match show {
        Ok(true) => {
            if let Err(e) = crate::window::create_onboarding_window(app) {
                tracing::error!(error=%e,"打开首次引导失败");
            }
            true
        }
        Err(e) => {
            tracing::error!(error=%e,"读取首次引导状态失败");
            false
        }
        Ok(false) => false,
    }
}

#[tauri::command]
pub async fn open_onboarding_window(app: AppHandle) -> Result<(), String> {
    #[cfg(mobile)]
    return Err("首次引导仅支持桌面端".into());
    #[cfg(desktop)]
    {
        let _guard = SESSION.lock().await;
        let db = db(&app)?;
        let step = local(&db, "onboarding_step")?;
        if step.as_deref() != Some("source_ready") && !can_open_manual_entry(&app, &db)? {
            return Err("已有提供商、助手或总管家配置，请在设置中调整。".into());
        }
        if app.get_webview_window("onboarding").is_none()
            && local(&db, "onboarding_state")?.as_deref() == Some("completed")
            && !matches!(step.as_deref(), Some("source_ready" | "awaiting_recharge"))
        {
            let tx = db.conn.unchecked_transaction().map_err(err)?;
            tx.execute("DELETE FROM system_config WHERE key IN ('onboarding_step','onboarding_path','onboarding_draft')",[]).map_err(err)?;
            tx.commit().map_err(err)?;
        }
        crate::window::create_onboarding_window(&app)
    }
}

#[tauri::command]
pub async fn choose_onboarding_path(
    app: AppHandle,
    window: tauri::WebviewWindow,
    path: String,
) -> Result<(), String> {
    require_window(&window)?;
    if !matches!(path.as_str(), "lie_flat" | "expert") {
        return Err("引导路径无效".into());
    }
    let _guard = SESSION.lock().await;
    let db = db(&app)?;
    if butler_configured(&db)? {
        return Err("管家已配置".into());
    }
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    if local(&db, "onboarding_state")?.as_deref() != Some("completed") {
        put(&tx, "onboarding_state", "in_progress")?;
    }
    put(&tx, "onboarding_path", &path)?;
    put(&tx, "onboarding_step", "path_chosen")?;
    tx.commit().map_err(err)
}

#[tauri::command]
pub async fn track_onboarding_provider(
    app: AppHandle,
    window: tauri::WebviewWindow,
    provider_id: i64,
    owned: bool,
) -> Result<(), String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let db = db(&app)?;
    let llm = LLMDatabase::new(&app).map_err(err)?;
    llm.get_llm_provider(provider_id).map_err(err)?;
    if !owned {
        release(&llm, provider_id)?;
    }
    let owned = owned && is_owned(&llm, provider_id)?;
    let mut d = draft(&db)?;
    if let Some(existing) = d.providers.iter_mut().find(|p| p.provider_id == provider_id) {
        if !owned {
            existing.owned = false;
        }
    } else {
        d.providers.push(DraftProvider { provider_id, owned });
    }
    save_draft(&db.conn, &d)
}

#[tauri::command]
pub async fn discard_onboarding_provider(
    app: AppHandle,
    window: tauri::WebviewWindow,
    provider_id: i64,
) -> Result<(), String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let db = db(&app)?;
    let mut d = draft(&db)?;
    if d.providers.iter().any(|p| p.provider_id == provider_id && p.owned)
        && Some(provider_id) != d.recharge_provider_id
    {
        remove_owned(&LLMDatabase::new(&app).map_err(err)?, provider_id)?;
        notify_provider_change(&app);
    }
    d.providers.retain(|p| p.provider_id != provider_id);
    let discarding_recharge = d.recharge_provider_id == Some(provider_id);
    if discarding_recharge {
        release(&LLMDatabase::new(&app).map_err(err)?, provider_id)?;
        d.recharge_provider_id = None;
    }
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    save_draft(&tx, &d)?;
    if discarding_recharge && local(&db, "onboarding_step")?.as_deref() == Some("awaiting_recharge")
    {
        put(&tx, "onboarding_step", "path_chosen")?;
    }
    tx.commit().map_err(err)
}

#[tauri::command]
pub async fn save_onboarding_recharge(
    app: AppHandle,
    window: tauri::WebviewWindow,
    provider_id: i64,
) -> Result<(), String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let db = db(&app)?;
    let mut d = draft(&db)?;
    if !d.providers.iter().any(|p| p.provider_id == provider_id) {
        return Err("待充值的提供商未登记".into());
    }
    let provider =
        LLMDatabase::new(&app).map_err(err)?.get_llm_provider(provider_id).map_err(err)?;
    if provider.api_type != "deepseek" {
        return Err("此提供商不支持余额恢复".into());
    }
    d.recharge_provider_id = Some(provider_id);
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    save_draft(&tx, &d)?;
    put(&tx, "onboarding_step", "awaiting_recharge")?;
    tx.commit().map_err(err)
}

#[tauri::command]
pub async fn skip_onboarding(app: AppHandle, window: tauri::WebviewWindow) -> Result<(), String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let db = db(&app)?;
    let mut d = draft(&db)?;
    let keep = d.recharge_provider_id;
    let llm = LLMDatabase::new(&app).map_err(err)?;
    if let Some(id) = keep {
        release(&llm, id)?;
    }
    let has_transient_providers =
        d.providers.iter().any(|p| p.owned && Some(p.provider_id) != keep);
    clear_unverified(&db, &llm, &mut d, keep)?;
    if has_transient_providers {
        notify_provider_change(&app);
    }
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    if local(&db, "onboarding_state")?.as_deref() != Some("completed") {
        put(&tx, "onboarding_state", "skipped")?;
    }
    tx.execute("DELETE FROM system_config WHERE key IN ('onboarding_draft','onboarding_path','onboarding_step')",[]).map_err(err)?;
    tx.commit().map_err(err)?;
    notify_state_change(&app);
    #[cfg(desktop)]
    {
        if let Some(w) = app.get_webview_window("chat_ui") {
            crate::window::open_chat_ui_window_inner(&app, &w);
        } else {
            crate::window::create_chat_ui_window(&app);
        }
        close_window(&app)?;
    }
    Ok(())
}

#[cfg(desktop)]
fn close_window(app: &AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("onboarding") {
        w.destroy().map_err(err)?;
    }
    Ok(())
}

// Opening the first home WebView from a synchronous IPC handler deadlocks WebView2 on Windows.
#[tauri::command(async)]
pub fn finish_onboarding(app: AppHandle, settings: Option<bool>) -> Result<(), String> {
    #[cfg(desktop)]
    {
        if local(&db(&app)?, "onboarding_state")?.as_deref() != Some("completed") {
            return Err("首次引导尚未完成".into());
        }
        if !settings.unwrap_or(false) {
            crate::window::open_default_home_window(&app);
        }
        close_window(&app)?;
    }
    Ok(())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncCheck {
    completed: bool,
    reason: Option<String>,
    settings_page: Option<String>,
}
#[tauri::command]
pub async fn check_onboarding_sync(
    app: AppHandle,
    window: tauri::WebviewWindow,
    installed_provider_ids: Vec<i64>,
) -> Result<SyncCheck, String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let keys_pending = crate::sync::onboarding_sync_keys_pending(&app).await?;
    let db = db(&app)?;
    let llm = LLMDatabase::new(&app).map_err(err)?;
    let (reason, page) = match sources(&llm, Some(&installed_provider_ids))? {
        Source::Usable => (None, None),
        Source::MissingKey if keys_pending => (
            Some("模型配置已同步，请在数据同步设置输入同步口令以解开 API Key。".into()),
            Some("data_sync".into()),
        ),
        Source::MissingKey => (
            Some("已同步的 API 提供商缺少 Key，请在模型提供商设置中填写。".into()),
            Some("model_provider".into()),
        ),
        Source::MissingAgent => (
            Some("智能体配置已同步，这台电脑尚未检测到对应的智能体。".into()),
            Some("model_provider".into()),
        ),
        Source::None => (
            Some("同步已结束，尚未找到已启用的聊天模型或本机智能体。".into()),
            Some("model_provider".into()),
        ),
    };
    let completed = reason.is_none() || butler_configured(&db)?;
    if completed {
        let mut pending = draft(&db)?;
        if let Some(saved) = pending.recharge_provider_id {
            release(&llm, saved)?;
        }
        clear_unverified(&db, &llm, &mut pending, None)?;
        let tx = db.conn.unchecked_transaction().map_err(err)?;
        complete(&tx)?;
        tx.execute("DELETE FROM system_config WHERE key IN ('onboarding_draft','onboarding_step','onboarding_path')",[]).map_err(err)?;
        tx.commit().map_err(err)?;
        notify_state_change(&app);
        if reason.is_none() {
            finish_onboarding(app, None)?;
        }
    }
    Ok(SyncCheck { completed, reason, settings_page: if completed { page } else { None } })
}

#[tauri::command]
pub fn get_default_new_chat_assistant(app: AppHandle) -> Result<Option<i64>, String> {
    let Some(id) = local(&db(&app)?, "default_new_chat_assistant_id")?.and_then(|s| s.parse().ok())
    else {
        return Ok(None);
    };
    match AssistantDatabase::new(&app).map_err(err)?.get_assistant(id) {
        Ok(_) => Ok(Some(id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(err(e)),
    }
}
#[tauri::command]
pub fn set_default_new_chat_assistant(app: AppHandle, assistant_id: i64) -> Result<(), String> {
    AssistantDatabase::new(&app).map_err(err)?.get_assistant(assistant_id).map_err(err)?;
    put(&db(&app)?.conn, "default_new_chat_assistant_id", &assistant_id.to_string())?;
    let _ = app.emit("default-new-chat-assistant-changed", assistant_id);
    Ok(())
}

#[tauri::command]
pub async fn reset_onboarding_state(app: AppHandle) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Err("重置首次引导仅用于调试构建".into());
    }
    let _guard = SESSION.lock().await;
    let db = db(&app)?;
    let mut d = draft(&db)?;
    let keep = d.recharge_provider_id;
    clear_unverified(&db, &LLMDatabase::new(&app).map_err(err)?, &mut d, keep)?;
    db.conn.execute("DELETE FROM system_config WHERE key IN ('onboarding_state','onboarding_path','onboarding_step','onboarding_draft','onboarding_completed_at')",[]).map_err(err)?;
    notify_state_change(&app);
    Ok(())
}

#[cfg(test)]
#[path = "state_tests.rs"]
mod state_tests;
