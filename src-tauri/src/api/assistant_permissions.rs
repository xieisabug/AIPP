//! Narrow one-time grants for Butler configuration tools; UI edits remain user actions.
use serde_json::{json, Value};
use crate::db::{assistant_db::AssistantDatabase, llm_db::LLMDatabase};

#[derive(Clone, Copy)]
pub(crate) enum Policy<'a> { Ordinary, Approved(&'a Value) }

fn path(value: &str) -> String {
    crate::utils::path_utils::normalize_path_for_comparison(std::path::Path::new(value))
        .map(|p|p.to_string_lossy().into_owned()).unwrap_or_else(||value.into())
}

pub(crate) fn state(db: &AssistantDatabase, llm: &LLMDatabase, id: i64) -> Result<Value,String> {
    let assistant = super::editable(db,id)?;
    let mut paths = db.get_assistant_workspaces(id).map_err(|e|e.to_string())?.into_iter().map(|w|path(&w.path)).collect::<Vec<_>>();
    paths.sort(); paths.dedup();
    let mut permissions = serde_json::Map::new();
    let operation_review = super::config_value(db,id,"tool_review_mode")?;
    permissions.insert("aipp_tool_review_mode".into(),json!(if operation_review.as_deref()==Some("auto_model") {"auto_model"}else{"off"}));
    if assistant.assistant_type==Some(4) {
        let channel = super::channel::read(db,llm,id)?;
        for field in channel["parameters"].as_array().ok_or("通道参数响应无效")? {
            let name = field["name"].as_str().unwrap_or("");
            if matches!(name,"acp_working_directory"|"codex_sandbox"|"codex_approval_policy"|"codex_approvals_reviewer"|"tool_review_mode") {
                let mut value = field["effective_value"].clone();
                if name=="acp_working_directory" { if let Some(text)=value.as_str() {value=json!(path(text));} }
                permissions.insert(name.into(),value);
            }
        }
    }
    let models = db.get_assistant_model(id).map_err(|e|e.to_string())?;
    let provider_id = if assistant.assistant_type==Some(4) {
        crate::api::assistant_api::resolve_acp_provider_id(&models,&db.get_assistant_model_configs(id).map_err(|e|e.to_string())?)
    } else {models.first().map(|m|m.provider_id)};
    let models = models.into_iter()
        .map(|m|json!({"id":m.id,"provider_id":m.provider_id,"model_code":m.model_code})).collect::<Vec<_>>();
    Ok(json!({"assistant_id":id,"assistant_type":assistant.assistant_type,"provider_id":provider_id,"models":models,"permissions":permissions,"workspace_paths":paths}))
}

fn covered(target: &str, trusted: &str) -> bool {
    crate::utils::path_utils::is_path_under_trusted(std::path::Path::new(target),std::path::Path::new(trusted))
}

pub(crate) fn expands(before: &Value, after: &Value) -> bool {
    // Review-mode switches change who decides; per-tool auto-run rules mean
    // neither direction is unconditionally a permission reduction.
    if ["aipp_tool_review_mode","tool_review_mode"].iter().any(|key|before["permissions"][key]!=after["permissions"][key]) {return true;}
    let paths = before["workspace_paths"].as_array().cloned().unwrap_or_default();
    if after["workspace_paths"].as_array().is_some_and(|new|new.iter().filter_map(Value::as_str)
        .any(|p|!paths.iter().filter_map(Value::as_str).any(|old|covered(p,old)))) {return true;}
    for (key,values) in [
        ("codex_sandbox",&["read-only","workspace-write","danger-full-access"][..]),
        ("codex_approval_policy",&["untrusted","on-request","never"][..]),
        ("codex_approvals_reviewer",&["user","auto_review"][..]),
    ] {
        let old = before["permissions"][key].as_str(); let new = after["permissions"][key].as_str();
        if old!=new {
            match (old.and_then(|v|values.iter().position(|x|*x==v)),new.and_then(|v|values.iter().position(|x|*x==v))) {
                (Some(a),Some(b)) if b<=a => {},
                _ => return true,
            }
        }
    }
    let old = before["permissions"]["acp_working_directory"].as_str();
    let new = after["permissions"]["acp_working_directory"].as_str();
    if before["permissions"]["codex_sandbox"]=="workspace-write" && after["permissions"]["codex_sandbox"]=="workspace-write"
        && old!=new && new.is_some_and(|new|!old.is_some_and(|old|covered(new,old))) {return true;}
    false
}

pub(crate) fn check_before(policy: Policy<'_>, before: &Value) -> Result<(),String> {
    if let Policy::Approved(expected) = policy {
        if expected.get("before")!=Some(before) {
            return Err(format!("助手 #{} 的权限配置在等待批准期间发生变化，请重新预览并请求批准；未修改任何配置",before["assistant_id"]));
        }
    }
    Ok(())
}

pub(crate) fn finish(policy: Policy<'_>, before: Value, after: Value, dry_run: bool, grant_action: &str) -> Result<Value,String> {
    let expansion = expands(&before,&after);
    match policy {
        Policy::Ordinary if expansion && !dry_run => return Err(format!("该操作扩大助手权限，未保存。请使用 {grant_action} 请求用户批准本次具体变更；模型审核或总管家不能代替用户批准")),
        Policy::Approved(expected) if expected.get("after")!=Some(&after) => return Err("批准的目标路径或权限已经变化，请重新预览并请求批准；未修改任何配置".into()),
        _ => {},
    }
    Ok(json!({"before":before,"after":after,"requires_user_approval":expansion,"approval_action":grant_action}))
}
