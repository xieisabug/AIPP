use super::*;
use crate::api::{assistant_api::management, butler_api, system_api::save_feature_config};
use crate::db::llm_db::is_local_agent_api_type;
use rusqlite::OptionalExtension;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use tauri::Emitter;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Role {
    provider_id: i64,
    code: String,
    verified: bool,
}
impl Role {
    fn selection(&self) -> String {
        format!("{}%%{}", self.code, self.provider_id)
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModels {
    provider_id: i64,
    codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoleNote {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

impl RoleNote {
    // Transport errors can echo a request URL or a credential. Only persist the category
    // shown by the wizard. The current window can still display the full diagnostic.
    fn sanitize(&mut self) {
        self.reason = self.reason.take().and_then(|reason| {
            if matches!(
                reason.as_str(),
                "自动选择" | "服务商推荐预设"
                    | "模型名称匹配轻量关键词"
                    | "此来源只有一个模型"
                    | "手动选择"
                    | "本机已安装智能体"
            ) {
                return Some(reason);
            }
            let rank =
                reason.strip_prefix("推荐表第 ")?.strip_suffix(" 名")?.parse::<u32>().ok()?;
            Some(format!("推荐表第 {rank} 名"))
        });
        if let Some(error) = &mut self.error {
            if error.starts_with("连不上 ") && error.ends_with("，检查网络或代理设置")
            {
                *error = "连不上服务商，检查网络或代理设置".into();
            } else if !matches!(
                error.as_str(),
                "这个 Key 不对，检查一下是否复制完整"
                    | "Key 没问题，但账户余额为 0，充值后才能对话"
                    | "这个服务商没有可用的推荐模型"
                    | "请求太频繁，稍等几秒再试"
                    | "连不上服务商，检查网络或代理设置"
            ) {
                *error = "模型检查未通过，可重新检查".into();
            }
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    chat: Role,
    light: Option<Role>,
    review: Option<Role>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    clear_light: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    clear_review: bool,
    butler: Option<Role>,
    home: String,
    workspace_path: Option<String>,
    workspace_description: Option<String>,
    trusted_paths: Option<Vec<String>>,
    trusted_workspaces_raw: Option<String>,
    display_name: Option<String>,
    #[serde(default)]
    models: Vec<ProviderModels>,
    #[serde(default)]
    extras: Vec<String>,
    #[serde(default)]
    confirm_channel_change: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    notes: BTreeMap<String, RoleNote>,
}
impl Config {
    fn remap(&mut self, targets: &BTreeMap<i64, i64>) {
        for role in
            [Some(&mut self.chat), self.light.as_mut(), self.review.as_mut(), self.butler.as_mut()]
                .into_iter()
                .flatten()
        {
            if let Some(id) = targets.get(&role.provider_id) {
                role.provider_id = *id;
            }
        }
        for models in &mut self.models {
            if let Some(id) = targets.get(&models.provider_id) {
                models.provider_id = *id;
            }
        }
    }
    fn validated_roles(&self) -> impl Iterator<Item = &Role> {
        [
            Some(&self.chat),
            self.light.as_ref(),
            self.review.as_ref(),
            self.butler.as_ref().filter(|_| self.home == "butler_experiment"),
        ]
        .into_iter()
        .flatten()
        .filter(|r| r.verified)
    }
    fn referenced_provider_ids(&self) -> Vec<i64> {
        [
            Some(&self.chat),
            self.light.as_ref(),
            self.review.as_ref(),
            self.butler.as_ref().filter(|_| self.home == "butler_experiment"),
        ]
        .into_iter()
        .flatten()
        .map(|r| r.provider_id)
        .collect()
    }
}

fn role_allowed(llm: &LLMDatabase, role: &Role, auxiliary: bool) -> Result<(), String> {
    let p = llm
        .get_llm_provider(role.provider_id)
        .map_err(|e| format!("读取提供商 #{} 失败：{e}", role.provider_id))?;
    if role.code.trim().is_empty() {
        return Err(format!("提供商「{}」未选择模型", p.name));
    }
    if is_local_agent_api_type(&p.api_type) {
        if auxiliary {
            return Err("轻量和审核角色需要 API 聊天模型".into());
        }
    } else {
        if !has_api_credentials(llm, &p)? {
            return Err(format!("提供商「{}」缺少 API Key，请检查配置", p.name));
        }
        let mode = llm.effective_model_mode(&p.api_type, p.id, &role.code).map_err(err)?;
        if p.api_type == "typesafe_jev" || crate::decision::is_decision_mode(&mode) {
            return Err(format!("模型 {} 不能用于聊天角色", role.code));
        }
    }
    Ok(())
}
fn validate(llm: &LLMDatabase, c: &Config) -> Result<(), String> {
    validate_clear(c.light.as_ref(), c.clear_light, c.review.as_ref(), c.clear_review)?;
    if let Some(id) =
        c.extras.iter().find(|id| !matches!(id.as_str(), "skills" | "office" | "bun" | "uv"))
    {
        return Err(format!("未知的附加组件：{id}"));
    }
    if !matches!(c.home.as_str(), "chat_ui" | "butler_experiment") {
        return Err("请选择聊天或管家首页".into());
    }
    if !c.chat.verified {
        return Err("聊天主模型尚未通过检查".into());
    }
    role_allowed(llm, &c.chat, false)?;
    if c.home == "butler_experiment" {
        let b = c.butler.as_ref().unwrap_or(&c.chat);
        if !b.verified {
            return Err("总管家模型尚未通过检查".into());
        }
        role_allowed(llm, b, false)?;
    }
    for role in [c.light.as_ref(), c.review.as_ref()].into_iter().flatten().filter(|r| r.verified) {
        role_allowed(llm, role, true)?;
    }
    Ok(())
}

fn seed_target(llm: &LLMDatabase, id: i64) -> Result<Option<i64>, String> {
    let p = llm.get_llm_provider(id).map_err(err)?;
    if p.is_official || is_local_agent_api_type(&p.api_type) {
        return Ok(None);
    }
    // Vault approval is bound to the original provider id. Leave such a provider in place.
    if llm.get_llm_provider_config(id).map_err(err)?.iter().any(|c| c.value.starts_with("vault://"))
    {
        return Ok(None);
    }
    llm.conn.query_row(
        "SELECT p.id FROM llm_provider p WHERE p.is_official=1 AND p.is_enabled=0 AND p.api_type=?1
         AND p.id IN (1,10,20,30)
         AND NOT EXISTS (SELECT 1 FROM llm_model m WHERE m.llm_provider_id=p.id)
         AND NOT EXISTS (SELECT 1 FROM llm_provider_config c WHERE c.llm_provider_id=p.id AND trim(c.value)<>'')
         ORDER BY p.id LIMIT 1",[&p.api_type],|r|r.get(0)).optional().map_err(err)
}

fn merge_seed(llm: &LLMDatabase, source: i64, target: i64) -> Result<(), String> {
    let provider = llm.get_llm_provider(source).map_err(err)?;
    let builtin = llm.get_llm_provider(target).map_err(err)?;
    if !builtin.is_official || builtin.api_type != provider.api_type {
        return Err("内置提供商发生变化，请重新选择来源".into());
    }
    let configs = llm.get_llm_provider_config(source).map_err(err)?;
    let prior = llm.get_llm_provider_config(target).map_err(err)?;
    for previous in prior.iter().filter(|c| !c.value.trim().is_empty()) {
        if !configs.iter().any(|c| {
            c.name == previous.name
                && c.value == previous.value
                && c.is_addition == previous.is_addition
                && c.append_location == previous.append_location
        }) {
            return Err("内置提供商的连接配置已变更，请重新选择来源".into());
        }
    }
    for config in configs {
        if config.value.starts_with("vault://") {
            return Err("保险箱引用须保留原提供商绑定".into());
        }
        if let Some(saved) = prior.iter().find(|c| c.name == config.name) {
            llm.conn.execute("UPDATE llm_provider_config SET value=?2,append_location=?3,is_addition=?4 WHERE id=?1",
                params![saved.id,config.value,config.append_location,config.is_addition]).map_err(err)?;
        } else {
            llm.add_llm_provider_config(
                target,
                &config.name,
                &config.value,
                &config.append_location,
                config.is_addition,
            )
            .map_err(err)?;
        }
    }
    llm.conn.execute("INSERT INTO llm_model (name,llm_provider_id,code,description,vision_support,audio_support,video_support)
        SELECT m.name,?2,m.code,m.description,m.vision_support,m.audio_support,m.video_support FROM llm_model m WHERE m.llm_provider_id=?1
        AND NOT EXISTS(SELECT 1 FROM llm_model t WHERE t.llm_provider_id=?2 AND t.code=m.code)",(source,target)).map_err(err)?;
    llm.conn.execute("INSERT OR REPLACE INTO llm_model_request_mode_preference(llm_provider_id,model_code,request_mode)
        SELECT ?2,model_code,request_mode FROM llm_model_request_mode_preference WHERE llm_provider_id=?1",(source,target)).map_err(err)?;
    Ok(())
}

fn prepare_providers(
    sys: &SystemDatabase,
    llm: &LLMDatabase,
    d: &mut Draft,
    c: &mut Config,
    retry_roles: Option<&[Role]>,
) -> Result<(), String> {
    let requested = retry_roles
        .map(|roles| roles.to_vec())
        .unwrap_or_else(|| c.validated_roles().cloned().collect());
    let sources: BTreeSet<_> = requested.iter().map(|r| r.provider_id).collect();
    for id in sources {
        if d.providers.iter().any(|p| p.provider_id == id && p.owned)
            && is_owned(llm, id)?
            && !d.seed_targets.contains_key(&id)
        {
            if let Some(target) = seed_target(llm, id)? {
                // Never merge two independently selected credentials into the same seed.
                if !d.seed_targets.values().any(|v| *v == target) {
                    d.seed_targets.insert(id, target);
                }
            }
        }
    }
    save_draft(&sys.conn, d)?;
    let tx = llm.conn.unchecked_transaction().map_err(err)?;
    for (&source, &target) in &d.seed_targets {
        if d.providers.iter().any(|p| p.provider_id == source && p.owned)
            && is_owned(llm, source)?
            && llm
                .conn
                .query_row("SELECT EXISTS(SELECT 1 FROM llm_provider WHERE id=?)", [source], |r| {
                    r.get::<_, bool>(0)
                })
                .map_err(err)?
        {
            merge_seed(llm, source, target)?;
        }
    }
    c.remap(&d.seed_targets);
    let mut selections: BTreeMap<i64, BTreeSet<String>> = BTreeMap::new();
    for r in &requested {
        let id = d.seed_targets.get(&r.provider_id).copied().unwrap_or(r.provider_id);
        selections.entry(id).or_default().insert(r.code.clone());
    }
    let lie_flat = local(sys, "onboarding_path")?.as_deref() == Some("lie_flat");
    for (id, mut codes) in selections {
        let p = llm.get_llm_provider(id).map_err(err)?;
        let deepseek_defaults = lie_flat && p.api_type == "deepseek";
        let first_activation = retry_roles.is_none() || (deepseek_defaults && !p.is_enabled);
        if first_activation {
            for models in c.models.iter().filter(|m| m.provider_id == id) {
                for code in models.codes.iter().filter(|value| !value.trim().is_empty()) {
                    if !deepseek_defaults && codes.len() >= 10 { break; }
                    codes.insert(code.clone());
                }
            }
            if deepseek_defaults {
                for key in ["deepseek_web_enabled", crate::api::deepseek_api::SHOW_BALANCE_IN_CHAT_CONFIG] {
                    llm.update_llm_provider_config(id, key, "true").map_err(err)?;
                    // The source survives until the next checkpoint. Keep it identical so merge_seed can replay.
                    for (&source, &target) in &d.seed_targets {
                        if target == id && is_owned(llm, source)? {
                            llm.update_llm_provider_config(source, key, "true").map_err(err)?;
                        }
                    }
                }
            }
        }
        if !is_local_agent_api_type(&p.api_type) {
            for code in codes {
                llm.conn.execute("INSERT INTO llm_model(llm_provider_id,code,name,description) SELECT ?1,?2,?2,'' WHERE NOT EXISTS
                    (SELECT 1 FROM llm_model WHERE llm_provider_id=?1 AND code=?2)",(id,code)).map_err(err)?;
            }
        }
        llm.conn.execute("UPDATE llm_provider SET is_enabled=1 WHERE id=?1", [id]).map_err(err)?;
        release(llm, id)?;
    }
    tx.commit().map_err(err)?;
    // Checkpoint normalized selections before removing their original rows. Both sides of
    // the checkpoint can therefore be replayed after a crash.
    if let Some(saved) = d.config.as_mut() {
        saved.remap(&d.seed_targets);
    }
    save_draft(&sys.conn, d)?;
    for source in d.seed_targets.keys() {
        if d.providers.iter().any(|p| p.owned && p.provider_id == *source) {
            remove_owned(llm, *source)?;
        }
    }
    d.providers.retain(|p| !d.seed_targets.contains_key(&p.provider_id));
    for role in c.validated_roles() {
        if let Some(p) = d.providers.iter_mut().find(|p| p.provider_id == role.provider_id) {
            p.owned = false;
        }
    }
    save_draft(&sys.conn, d)
}

fn bind_chat(app: &AppHandle, llm: &LLMDatabase, c: &Config) -> Result<i64, String> {
    let p = llm.get_llm_provider(c.chat.provider_id).map_err(err)?;
    if !is_local_agent_api_type(&p.api_type) {
        management::profile_in_app(
            app,
            "update",
            &serde_json::json!({"assistant_id":1,"provider_id":p.id,"model_code":c.chat.code}),
            false,
        )?;
        return Ok(1);
    }
    let choices = llm.get_filtered_models_for_select(4)?;
    let (_, _, model_id, _) = choices
        .iter()
        .find(|(_, code, _, id)| *id == p.id && code == &c.chat.code)
        .ok_or("所选智能体模型不在本机提供商模型列表中")?;
    let code = if *model_id == 0 { "" } else { &c.chat.code };
    let adb = AssistantDatabase::new(app).map_err(err)?;
    let ids = adb.get_assistants().map_err(err)?;
    for assistant in ids {
        if assistant.assistant_type != Some(4) || butler_api::is_butler_system_assistant(&assistant)
        {
            continue;
        }
        if adb
            .get_assistant_model(assistant.id)
            .map_err(err)?
            .first()
            .is_some_and(|m| m.provider_id == p.id && m.model_code == code)
        {
            return Ok(assistant.id);
        }
    }
    let result = management::profile_in_app(
        app,
        "create",
        &serde_json::json!({"name":p.name,"assistant_type":4,"provider_id":p.id,"model_code":code}),
        false,
    )?;
    result["assistant_id"].as_i64().ok_or_else(|| "创建智能体助手未返回编号".into())
}

fn apply_chat_defaults(db: &AssistantDatabase, assistant: i64, output: u64) -> Result<(), String> {
    let model = db.get_assistant_model(assistant).map_err(err)?.into_iter().next()
        .ok_or("聊天助手缺少模型绑定")?;
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    let output = output.to_string();
    let mut options = vec![
        ("max_tokens", output.as_str(), "number"), ("stream", "true", "boolean"),
        ("mcp_loading_mode_explicit", "true", "boolean"), ("dynamic_mcp_loading_enabled", "true", "boolean"),
    ];
    if assistant == 1 {
        options.push(("tool_review_mode", "auto_model", "string"));
    }
    for (name, value, kind) in options {
        tx.execute("INSERT INTO assistant_model_config(assistant_id,assistant_model_id,name,value,value_type)
            VALUES(?1,?2,?3,?4,?5) ON CONFLICT(assistant_id,name) DO UPDATE SET
            assistant_model_id=excluded.assistant_model_id,value=excluded.value,value_type=excluded.value_type",
            params![assistant,model.id,name,value,kind]).map_err(err)?;
    }
    tx.commit().map_err(err)
}

fn apply_quick_appearance(sys: &SystemDatabase, db: &AssistantDatabase) -> Result<(), String> {
    let key = "butler_appearance_1";
    let legacy = sys.get_feature_config("butler_appearance", key).map_err(err)?
        .is_some_and(|value| !value.value.trim().is_empty());
    let appearance = serde_json::json!({
        "version": 2, "shape": "aipp", "colors": {"primary": "#FFFFFF", "secondary": "#000000"},
        "accessories": {"eyes": null, "head": null}, "motion": true
    });
    db.conn.execute("INSERT INTO assistant_appearance(assistant_id,value)
        SELECT 1,?1 WHERE NOT ?2 OR EXISTS(SELECT 1 FROM assistant_appearance_foreign_legacy WHERE key=?3)
        ON CONFLICT(assistant_id) DO NOTHING", params![appearance.to_string(), legacy, key]).map_err(err)?;
    Ok(())
}

fn validate_clear(
    light: Option<&Role>,
    clear_light: bool,
    review: Option<&Role>,
    clear_review: bool,
) -> Result<(), String> {
    if (light.is_some() && clear_light) || (review.is_some() && clear_review) {
        return Err("同一角色不能同时指定模型和清空配置".into());
    }
    Ok(())
}

fn role_settings(
    light: Option<&Role>,
    review: Option<&Role>,
    clear_light: bool,
    clear_review: bool,
    lie_flat: bool,
) -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Some(role) = light.filter(|r| r.verified) {
        map.insert("default_light_model".into(), role.selection());
    }
    if let Some(role) = review.filter(|r| r.verified) {
        if lie_flat {
            map.insert("default_decision_model".into(), role.selection());
            map.insert("auto_review_model".into(), String::new());
            map.insert("auto_review_provider_id".into(), String::new());
        } else {
            map.insert("auto_review_model".into(), role.code.clone());
            map.insert("auto_review_provider_id".into(), role.provider_id.to_string());
        }
    }
    if clear_light {
        map.insert("default_light_model".into(), String::new());
    }
    if clear_review {
        if lie_flat {
            map.insert("default_decision_model".into(), String::new());
        }
        map.insert("auto_review_model".into(), String::new());
        map.insert("auto_review_provider_id".into(), String::new());
    }
    map
}

// Clear requests are one-shot. Persist them in the recovery checkpoint, then
// remove them from the finished configuration so a later edit cannot erase a new setting.
fn finish_role_clear(config: &mut Config) {
    if config.clear_light {
        config.light = None;
        config.notes.remove("light");
    }
    if config.clear_review {
        config.review = None;
        config.notes.remove("review");
    }
    config.clear_light = false;
    config.clear_review = false;
}
async fn partial(app: &AppHandle, group: &str, map: HashMap<String, String>) -> Result<(), String> {
    if map.is_empty() {
        return Ok(());
    }
    save_feature_config(app.clone(), app.state(), group.into(), map, Some(true)).await
}

fn home_settings(
    sys: &SystemDatabase,
    c: &Config,
    default_path: impl FnOnce() -> Result<String, String>,
) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::from([("default_home_window".into(), c.home.clone())]);
    if c.home == "butler_experiment" {
        let existing = experiment(sys, "butler_main_workspace_path")?;
        let state = local(sys, "onboarding_state")?;
        let path = if workspace_locked(state.as_deref(), &existing) {
            existing
        } else if let Some(p) = c.workspace_path.as_ref().filter(|p| !p.trim().is_empty()) {
            p.trim().into()
        } else if !existing.trim().is_empty() {
            existing
        } else {
            default_path()?
        };
        if !std::path::Path::new(&path).is_absolute() {
            return Err(format!("请选择绝对的主工作区路径：{path}"));
        }
        map.insert("butler_experiment_enabled".into(), "true".into());
        map.insert("butler_model_id".into(), c.butler.as_ref().unwrap_or(&c.chat).selection());
        map.insert("butler_main_workspace_path".into(), path);
        if let Some(desc) = &c.workspace_description {
            map.insert("butler_main_workspace_description".into(), desc.clone());
        } else if experiment(sys, "butler_main_workspace_description")?.trim().is_empty() {
            map.insert(
                "butler_main_workspace_description".into(),
                butler_api::BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION.into(),
            );
        }
        if let Some(raw) = &c.trusted_workspaces_raw {
            map.insert("butler_trusted_workspaces".into(), raw.clone());
        } else if let Some(paths) = &c.trusted_paths {
            map.insert("butler_trusted_workspaces".into(), paths.join("\n"));
        }
        if let Some(name) = &c.display_name {
            map.insert("butler_display_name".into(), name.trim().into());
        }
    }
    Ok(map)
}

#[tauri::command]
pub async fn apply_onboarding_config(
    app: AppHandle,
    window: tauri::WebviewWindow,
    config: Config,
) -> Result<Status, String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let sys = db(&app)?;
    let llm = LLMDatabase::new(&app).map_err(err)?;
    let mut d = draft(&sys)?;
    let step = local(&sys, "onboarding_step")?;
    if step.as_deref() == Some("source_ready") && d.pending_role_change.is_some() {
        return finish_role_update(app, sys, llm, d).await;
    }
    if !matches!(
        step.as_deref(),
        Some("path_chosen" | "awaiting_recharge" | "source_ready" | "applied")
    ) {
        return Err("请先选择首次引导路径".into());
    }
    if !matches!(step.as_deref(), Some("source_ready" | "applied")) && butler_configured(&sys)? {
        return Err("管家已配置，请在对应设置页调整".into());
    }
    // When recovering, only the on-disk selections are authoritative.
    let mut c = if step.as_deref() == Some("source_ready") {
        let mut saved = d.config.clone().ok_or("配置恢复记录缺少模型选择")?;
        saved.confirm_channel_change |= config.confirm_channel_change;
        saved
    } else {
        config
    };
    c.notes.retain(|key, _| matches!(key.as_str(), "chat" | "light" | "review"));
    for note in c.notes.values_mut() {
        note.sanitize();
    }
    // A repeated submit can still refer to the original id of a now-merged draft.
    for (&source, &target) in &d.seed_targets {
        if !d.providers.iter().any(|p| p.provider_id == source && p.owned) {
            c.remap(&BTreeMap::from([(source, target)]));
        }
    }
    if step.as_deref() == Some("applied") && d.config.as_ref() == Some(&c) {
        return get_onboarding_state(app);
    }
    if step.as_deref() == Some("applied") {
        d.previous_config = d.config.clone();
    }
    let previous = d.previous_config.clone();
    validate(&llm, &c)?;
    let changed_butler = previous.as_ref().is_none_or(|old| {
        old.home != c.home
            || old.butler.as_ref().unwrap_or(&old.chat) != c.butler.as_ref().unwrap_or(&c.chat)
    });
    if c.home == "butler_experiment" && changed_butler {
        let previous = experiment(&sys, "butler_model_id")?;
        let next = c.butler.as_ref().unwrap_or(&c.chat).selection();
        if butler_api::butler_channel_changed(&llm, &previous, &next)? {
            if !c.confirm_channel_change {
                return Err("butler_channel_change_confirmation_required".into());
            }
        }
    }
    d.config = Some(c.clone());
    {
        let tx = sys.conn.unchecked_transaction().map_err(err)?;
        save_draft(&tx, &d)?;
        put(&tx, "onboarding_step", "source_ready")?;
        tx.commit().map_err(err)?;
    }
    let changed_roles: Option<Vec<Role>> = previous.as_ref().map(|old| {
        c.validated_roles()
            .filter(|role| !old.validated_roles().any(|prior| prior == *role))
            .cloned()
            .collect()
    });
    prepare_providers(&sys, &llm, &mut d, &mut c, changed_roles.as_deref())?;
    let lie_flat = local(&sys, "onboarding_path")?.as_deref() == Some("lie_flat");
    let mut defaults = HashMap::new();
    if lie_flat && previous.is_none() {
        defaults.insert("conversation_summary_enabled".into(), "true".into());
    }
    if lie_flat && changed_butler && c.home == "butler_experiment" {
        let chief = c.butler.as_ref().unwrap_or(&c.chat);
        let provider = llm.get_llm_provider(chief.provider_id).map_err(err)?;
        let budget = preset::selected_budget(&sys, &provider.api_type, &chief.code)?;
        defaults.insert("butler_max_output_tokens".into(), budget.output.to_string());
    }
    if previous.as_ref().is_none_or(|old| old.chat != c.chat) {
        let assistant_id = bind_chat(&app, &llm, &c)?;
        if lie_flat {
            let provider = llm.get_llm_provider(c.chat.provider_id).map_err(err)?;
            let budget = preset::selected_budget(&sys, &provider.api_type, &c.chat.code)?;
            let db = AssistantDatabase::new(&app).map_err(err)?;
            apply_chat_defaults(&db, assistant_id, budget.output)?;
            apply_quick_appearance(&sys, &db)?;
            if assistant_id != 1 {
                // Ask continues to use the quick assistant when the main chat uses a native agent.
                let quick = db.get_assistant_model(1).map_err(err)?.into_iter().next().ok_or("快速使用助手缺少模型绑定")?;
                let quick_type = if quick.provider_id > 0 {
                    llm.get_llm_provider(quick.provider_id).map_err(err)?.api_type
                } else { String::new() };
                let quick_budget = preset::selected_budget(&sys, &quick_type, &quick.model_code)?;
                apply_chat_defaults(&db, 1, quick_budget.output)?;
            }
            defaults.insert("dynamic_mcp_loading_enabled".into(), "true".into());
            defaults.insert("context_compaction_enabled".into(), "true".into());
            defaults.insert("context_max_input_tokens".into(), budget.context.to_string());
            defaults.insert("context_output_reserve".into(), budget.output.min(budget.context / 2).to_string());
        }
        put(&sys.conn, "default_new_chat_assistant_id", &assistant_id.to_string())?;
        let _ = app.emit("assistant_list_changed", ());
        let _ = app.emit("default-new-chat-assistant-changed", assistant_id);
    }
    let _ = app.emit("llm_provider_changed", ());
    let light = c
        .light
        .as_ref()
        .filter(|role| previous.as_ref().and_then(|old| old.light.as_ref()) != Some(*role));
    let review = c
        .review
        .as_ref()
        .filter(|role| previous.as_ref().and_then(|old| old.review.as_ref()) != Some(*role));
    partial(
        &app,
        "conversation_summary",
        role_settings(light, review, c.clear_light, c.clear_review, lie_flat),
    )
    .await?;
    let mut home = home_settings(&sys, &c, || default_workspace(&app))?;
    if let Some(old) = previous.as_ref() {
        let prior = home_settings(&sys, old, || default_workspace(&app))?;
        home.retain(|k, v| prior.get(k) != Some(v));
    }
    home.extend(defaults);
    if let Some(path) = home.get("butler_main_workspace_path") {
        tokio::fs::create_dir_all(path)
            .await
            .map_err(|e| format!("创建管家主工作区 {path} 失败：{e}"))?;
    }
    partial(&app, "experimental", home).await?;
    if c.home == "butler_experiment" {
        // The settings service journals the old main conversation in the same transaction
        // as the new channel. A restart can safely finish that conditional reset here.
        butler_api::finish_pending_channel_change(&app).await?;
    }
    let keep = c.referenced_provider_ids();
    let _ = clear_unverified_except(&sys, &llm, &mut d, &keep)?;
    let tx =
        rusqlite::Transaction::new_unchecked(&sys.conn, rusqlite::TransactionBehavior::Immediate)
            .map_err(err)?;
    let queued = setup::enqueue_selection(
        &tx,
        &c.extras,
        previous.as_ref().map(|old| old.extras.as_slice()),
    )?;
    d.recharge_provider_id = None;
    d.previous_config = None;
    finish_role_clear(&mut c);
    d.config = Some(c);
    save_draft(&tx, &d)?;
    put(&tx, "onboarding_step", "applied")?;
    complete(&tx)?;
    tx.commit().map_err(err)?;
    notify_provider_change(&app);
    notify_state_change(&app);
    if let Some(items) = queued {
        let _ = app.emit("onboarding-setup-progress", items);
        setup::resume(&app);
    }
    get_onboarding_state(app)
}

#[tauri::command]
pub async fn update_onboarding_roles(
    app: AppHandle,
    window: tauri::WebviewWindow,
    light: Option<Role>,
    review: Option<Role>,
    clear_light: Option<bool>,
    clear_review: Option<bool>,
    notes: Option<BTreeMap<String, RoleNote>>,
) -> Result<Status, String> {
    require_window(&window)?;
    let _guard = SESSION.lock().await;
    let sys = db(&app)?;
    let llm = LLMDatabase::new(&app).map_err(err)?;
    let mut d = draft(&sys)?;
    if local(&sys, "onboarding_step")?.as_deref() != Some("applied") {
        return Err("请先完成主模型配置".into());
    }
    let mut c = d.config.clone().ok_or("首次引导没有已应用的模型分工")?;
    let clear_light = clear_light.unwrap_or(false);
    let clear_review = clear_review.unwrap_or(false);
    validate_clear(light.as_ref(), clear_light, review.as_ref(), clear_review)?;
    for role in [light.as_ref(), review.as_ref()].into_iter().flatten() {
        if !role.verified {
            return Err("辅助角色尚未通过检查，已保留原配置".into());
        }
        role_allowed(&llm, role, true)?;
    }
    c.clear_light = clear_light;
    c.clear_review = clear_review;
    if clear_light {
        c.light = None;
        c.notes.remove("light");
    }
    if clear_review {
        c.review = None;
        c.notes.remove("review");
    }
    for (key, chosen, slot) in
        [("light", light.as_ref(), &mut c.light), ("review", review.as_ref(), &mut c.review)]
    {
        if let Some(chosen) = chosen {
            let same_model = slot.as_ref().is_some_and(|old| {
                old.code == chosen.code && old.provider_id == chosen.provider_id
            });
            let reason = notes
                .as_ref()
                .and_then(|input| input.get(key))
                .and_then(|note| note.reason.clone())
                .or_else(|| {
                    if same_model {
                        c.notes.get(key).and_then(|old| old.reason.clone())
                    } else {
                        Some("手动选择".into())
                    }
                });
            let mut note = RoleNote { reason, error: None };
            note.sanitize();
            c.notes.insert(key.into(), note);
            *slot = Some(chosen.clone());
        }
    }
    // A restart must replay only these roles, even if the chat provider or home changed.
    d.pending_role_change = Some(PendingRoleChange {
        light: light.is_some() || clear_light,
        review: review.is_some() || clear_review,
    });
    d.config = Some(c);
    {
        let tx = sys.conn.unchecked_transaction().map_err(err)?;
        save_draft(&tx, &d)?;
        put(&tx, "onboarding_step", "source_ready")?;
        tx.commit().map_err(err)?;
    }
    finish_role_update(app, sys, llm, d).await
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(super) struct PendingRoleChange {
    light: bool,
    review: bool,
}

fn prepare_role_update(
    sys: &SystemDatabase,
    llm: &LLMDatabase,
    d: &mut Draft,
    c: &mut Config,
) -> Result<HashMap<String, String>, String> {
    let change = d.pending_role_change.ok_or("缺少待恢复的角色改动")?;
    let requested: Vec<_> = [
        change.light.then_some(c.light.as_ref()).flatten(),
        change.review.then_some(c.review.as_ref()).flatten(),
    ]
    .into_iter()
    .flatten()
    .cloned()
    .collect();
    for role in &requested {
        if !role.verified {
            return Err("辅助角色尚未通过检查，已保留原配置".into());
        }
        role_allowed(llm, role, true)?;
    }
    if !requested.is_empty() {
        prepare_providers(sys, llm, d, c, Some(&requested))?;
    }
    Ok(role_settings(
        change.light.then_some(c.light.as_ref()).flatten(),
        change.review.then_some(c.review.as_ref()).flatten(),
        change.light && c.clear_light,
        change.review && c.clear_review,
        local(sys, "onboarding_path")?.as_deref() == Some("lie_flat"),
    ))
}

async fn finish_role_update(
    app: AppHandle,
    sys: SystemDatabase,
    llm: LLMDatabase,
    mut d: Draft,
) -> Result<Status, String> {
    let mut c = d.config.clone().ok_or("角色恢复记录缺少模型分工")?;
    let changes = prepare_role_update(&sys, &llm, &mut d, &mut c)?;
    partial(&app, "conversation_summary", changes).await?;
    finish_role_clear(&mut c);
    d.config = Some(c);
    d.pending_role_change = None;
    {
        let tx = sys.conn.unchecked_transaction().map_err(err)?;
        save_draft(&tx, &d)?;
        put(&tx, "onboarding_step", "applied")?;
        tx.commit().map_err(err)?;
    }
    let _ = app.emit("llm_provider_changed", ());
    crate::sync::schedule_sync_after_local_change(&app);
    get_onboarding_state(app)
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
