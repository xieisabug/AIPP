use crate::{
    api::{ai::config::get_network_proxy_from_config, office_api, skill_api},
    artifacts::env_installer::{install_runtime, RuntimeTool},
    db::system_db::SystemDatabase,
    FeatureConfigState,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

const KEY: &str = "onboarding_setup_status";
const ORDER: [&str; 4] = ["skills", "office", "bun", "uv"];
static RUNNER: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Serialize, Deserialize)]
pub struct SetupItem {
    id: String,
    state: State,
    message: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum State { Pending, Running, Done, Skipped, Failed }

fn read(conn: &rusqlite::Connection) -> Result<Vec<SetupItem>, String> {
    use rusqlite::OptionalExtension;
    let raw: Option<String> = conn.query_row("SELECT value FROM system_config WHERE key=?1", [KEY], |row| row.get(0))
        .optional().map_err(|e| e.to_string())?;
    match raw.filter(|s| !s.is_empty()) {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_str(&value).map_err(|e| format!("读取准备任务状态失败：{e}")),
    }
}

fn persist(db: &SystemDatabase, edit: impl FnOnce(&mut Vec<SetupItem>) -> Result<(), String>) -> Result<Vec<SetupItem>, String> {
    let tx = rusqlite::Transaction::new_unchecked(&db.conn, rusqlite::TransactionBehavior::Immediate).map_err(|e| e.to_string())?;
    let items = persist_in(&tx, edit)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(items)
}

fn persist_in(tx: &rusqlite::Transaction<'_>, edit: impl FnOnce(&mut Vec<SetupItem>) -> Result<(), String>) -> Result<Vec<SetupItem>, String> {
    let mut items = read(tx)?;
    edit(&mut items)?;
    let raw = serde_json::to_string(&items).map_err(|e| e.to_string())?;
    tx.execute("INSERT INTO system_config(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        rusqlite::params![KEY,raw]).map_err(|e| e.to_string())?;
    Ok(items)
}

fn change(app: &AppHandle, edit: impl FnOnce(&mut Vec<SetupItem>) -> Result<(), String>) -> Result<(), String> {
    let db = SystemDatabase::new(app).map_err(|e| e.to_string())?;
    let items = persist(&db, edit)?;
    let _ = app.emit("onboarding-setup-progress", &items);
    Ok(())
}

fn include_selected(items: &mut Vec<SetupItem>, selected: &[String]) -> Result<(), String> {
    for id in selected {
        if !ORDER.contains(&id.as_str()) { return Err(format!("未知准备任务：{id}")); }
    }
    for id in ORDER {
        if selected.iter().any(|s| s == id) {
            if let Some(item) = items.iter_mut().find(|i| i.id == id) {
                if matches!(item.state, State::Done | State::Skipped) {
                    item.state = State::Pending;
                    item.message.clear();
                }
            } else {
                items.push(SetupItem { id: id.into(), state: State::Pending, message: String::new() });
            }
        }
    }
    items.sort_by_key(|i| ORDER.iter().position(|id| id == &i.id).unwrap_or(usize::MAX));
    Ok(())
}

/// Finishing a fresh wizard rechecks each selected component. Edits on its result page
/// enqueue only newly selected extras. The same transaction marks this apply as completed.
pub(super) fn enqueue_selection(
    tx: &rusqlite::Transaction<'_>, selected: &[String], previous: Option<&[String]>,
) -> Result<Option<Vec<SetupItem>>, String> {
    let requested: Vec<_> = selected.iter().filter(|id| !previous.is_some_and(|prior| prior.contains(id))).cloned().collect();
    if requested.is_empty() { return Ok(None); }
    persist_in(tx, |items| include_selected(items, &requested)).map(Some)
}

#[tauri::command]
pub fn get_onboarding_setup_status(app: AppHandle) -> Result<Vec<SetupItem>, String> {
    read(&SystemDatabase::new(&app).map_err(|e| e.to_string())?.conn)
}

#[tauri::command]
pub fn retry_onboarding_setup(app: AppHandle, id: String) -> Result<(), String> {
    if cfg!(mobile) { return Err("首次引导仅支持桌面端".into()); }
    change(&app, |items| {
        let item = items.iter_mut().find(|i| i.id == id).ok_or("准备任务不存在")?;
        if item.state == State::Failed { item.state = State::Pending; item.message.clear(); }
        Ok(())
    })?;
    resume(&app);
    Ok(())
}

pub(crate) fn resume(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let _guard = RUNNER.lock().await;
        if let Err(error) = run_pending(&app).await { tracing::error!(%error, "Onboarding setup queue failed"); }
    });
}

async fn run_pending(app: &AppHandle) -> Result<(), String> {
    loop {
        let items = get_onboarding_setup_status(app.clone())?;
        let Some(item) = items.iter().find(|i| matches!(i.state, State::Pending | State::Running)) else { return Ok(()) };
        let set = |state, message: String| change(app, |items| {
            let row = items.iter_mut().find(|i| i.id == item.id).ok_or("准备任务不存在")?;
            row.state = state;
            row.message = message;
            Ok(())
        });
        set(State::Running, String::new())?;
        let result = match item.id.as_str() {
            "skills" => install_skills(app).await,
            "office" => prepare_office(app).await,
            "bun" => install_runtime(app, RuntimeTool::Bun, None, true).await,
            "uv" => install_runtime(app, RuntimeTool::Uv, None, true).await,
            id => Err(format!("未知准备任务：{id}")),
        };
        match result {
            Ok(true) => set(State::Skipped, "已就绪".into())?,
            Ok(false) => set(State::Done, "已安装".into())?,
            Err(message) => set(State::Failed, message)?,
        }
    }
}

async fn prepare_office(app: &AppHandle) -> Result<bool, String> {
    let current = office_api::get_office_preview_plugin_status(app.clone()).await?;
    if current.viewer_ready && current.edit_supported && current.enabled && !current.update_required { return Ok(true); }
    let status = office_api::install_office_preview_plugin(app.clone(), None, None, Some(true)).await?;
    if !(status.viewer_ready && status.edit_supported && status.enabled) {
        return Err("Office 插件安装后未通过编辑能力检查".into());
    }
    Ok(false)
}

#[cfg(mobile)]
async fn install_skills(_: &AppHandle) -> Result<bool, String> { Err("首次引导仅支持桌面端".into()) }

#[cfg(desktop)]
async fn official_skill_sources(app: &AppHandle, proxy: bool, cache_preset: bool) -> Result<Vec<(
    String, crate::skills::installer::SkillInstallRecipeSource, Vec<crate::skills::installer::SkillInstallRecipeDir>,
)>, String> {
    let catalog = skill_api::fetch_official_skills(app.clone(), proxy).await?;
    let preset = super::preset::load_preset(app.clone(), cache_preset).await?;
    onboarding_skill_packages(catalog, &preset)?.into_iter().map(|package| {
        Ok((package.name.clone(), package.resolve_source()?, package.dirs))
    }).collect()
}

#[cfg(desktop)]
fn onboarding_skill_packages(catalog: Vec<skill_api::OfficialSkill>, preset: &serde_json::Value)
    -> Result<[skill_api::OfficialSkill; 2], String>
{
    let aipp = catalog.into_iter().find(|skill| skill.id == "aipp-skills")
        .ok_or("官方 Skills 目录缺少 AIPP Skills")?;
    // The marketplace entry for Vercel covers the whole repository. Install only find-skills.
    let find = preset["extra_skills"].as_array().into_iter().flatten()
        .find(|entry| entry["id"] == "vercel-labs-skills-find-skills").ok_or("引导预设缺少 find-skills")?;
    let mut find: skill_api::OfficialSkill = serde_json::from_value(find.clone()).map_err(|e| format!("find-skills 配置无效：{e}"))?;
    find.dirs.retain(|dir| dir.from == "skills/find-skills" && dir.to == "find-skills");
    if find.dirs.len() != 1 { return Err("引导预设缺少 find-skills 安装目录".into()); }
    Ok([aipp, find])
}

#[tauri::command]
pub async fn check_onboarding_skills_ready(app: AppHandle) -> Result<bool, String> {
    #[cfg(mobile)]
    { let _ = app; Err("首次引导仅支持桌面端".into()) }
    #[cfg(desktop)]
    {
        let dir = skill_api::skills_directory(&app)?;
        match std::fs::read_dir(&dir) {
            Ok(mut entries) => {
                if entries.next().transpose().map_err(|e| format!("枚举 Skills 目录失败：{e}"))?.is_none() { return Ok(false); }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("读取 Skills 目录失败：{e}")),
        }
        use crate::skills::installer::{official_skills_ready, read_official_archive_manifest, OfficialSkillEntry};
        use std::time::{Duration, Instant};
        // Cache the remote package inventory, never the state of a local installation.
        // Concurrent window refreshes share a download. TTL also covers mutable archive refs.
        static INVENTORY: tokio::sync::Mutex<Option<(Instant, Vec<OfficialSkillEntry>)>> =
            tokio::sync::Mutex::const_new(None);
        let mut inventory = INVENTORY.lock().await;
        if let Some((_, entries)) = inventory.as_ref().filter(|(at, _)| at.elapsed() < Duration::from_secs(300)) {
            return official_skills_ready(entries, &dir);
        }
        let features = app.state::<FeatureConfigState>();
        let proxy = get_network_proxy_from_config(&*features.config_feature_map.lock().await);
        let packages = official_skill_sources(&app, proxy.is_some(), false).await?;
        let mut entries = Vec::new();
        for (name, source, dirs) in packages {
            entries.extend(read_official_archive_manifest(&source, &dirs, &dir, proxy.as_deref())
                .await.map_err(|e| format!("{name}：{e}"))?);
        }
        let result = official_skills_ready(&entries, &dir);
        *inventory = Some((Instant::now(), entries));
        result
    }
}

#[cfg(desktop)]
async fn install_skills(app: &AppHandle) -> Result<bool, String> {
    let features = app.state::<FeatureConfigState>();
    let proxy = get_network_proxy_from_config(&*features.config_feature_map.lock().await);
    let packages = official_skill_sources(app, proxy.is_some(), true).await?;
    let dir = skill_api::skills_directory(app)?;
    let mut errors = Vec::new();
    let mut installed = 0;
    let mut targets = Vec::new();
    for (name, source, dirs) in packages {
        match crate::skills::installer::install_missing_archive(&source, Some(&dirs), &dir, proxy.as_deref()).await {
            Ok((count, paths)) => { installed += count; targets.extend(paths); }
            Err(error) => errors.push(format!("{name}：{error}")),
        }
    }
    let scanned = skill_api::scan_skills(app.clone()).await?;
    if !errors.is_empty() { return Err(errors.join("\n")); }
    for target in targets {
        let folder = dir.join(&target);
        if !scanned.iter().any(|s| std::path::Path::new(&s.file_path).parent() == Some(folder.as_path())) {
            return Err(format!("快速使用助手未找到已安装的 Skill：{target}"));
        }
    }
    crate::mcp::registry_api::ensure_agent_load_skill_for_assistant(app, 1)?;
    let db = crate::db::skill_db::SkillDatabase::new(app).map_err(super::err)?;
    let tx = db.conn.unchecked_transaction().map_err(super::err)?;
    for skill in scanned.iter().filter(|s| s.exists) {
        tx.execute("INSERT INTO assistant_skill_config(assistant_id,skill_identifier,is_enabled,priority)
            VALUES(1,?1,1,0) ON CONFLICT(assistant_id,skill_identifier) DO UPDATE SET is_enabled=1",
            [&skill.identifier]).map_err(super::err)?;
    }
    tx.commit().map_err(super::err)?;
    crate::sync::schedule_sync_after_local_change(app);
    Ok(installed == 0)
}

#[cfg(test)]
#[path = "setup_tests.rs"]
mod tests;
