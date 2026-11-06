use super::{is_butler_system_assistant, Assistant, AssistantDatabase, ButlerModelSelection,
    BUTLER_SYSTEM_ASSISTANT_DESCRIPTION, BUTLER_SYSTEM_ASSISTANT_NAME};
use rusqlite::{params, Result, Transaction, TransactionBehavior};

// 总管家不在助手配置页展示，工具审核模式固定为模型自动审核（未配置审核模型时会转为人工确认）。
const CONFIG: [(&str, &str, &str); 6] = [
    ("max_tokens", "16000", "number"), ("temperature", "0.8", "float"),
    ("top_p", "1.0", "float"), ("stream", "true", "boolean"),
    ("use_native_toolcall", "true", "boolean"), ("tool_review_mode", "auto_model", "string"),
];

fn current(db: &AssistantDatabase) -> Result<Option<Assistant>> {
    Ok(db.get_assistants()?.into_iter().find(is_butler_system_assistant))
}

fn matches(db: &AssistantDatabase, assistant: &Assistant, model: &ButlerModelSelection, prompt: &str) -> Result<bool> {
    let prompts = db.get_assistant_prompt(assistant.id)?;
    let models = db.get_assistant_model(assistant.id)?;
    let configs = db.get_assistant_model_configs(assistant.id)?;
    Ok(assistant.name == BUTLER_SYSTEM_ASSISTANT_NAME
        && assistant.description.as_deref() == Some(BUTLER_SYSTEM_ASSISTANT_DESCRIPTION)
        && prompts.len() == 1 && prompts[0].prompt == prompt
        && models.len() == 1 && models[0].provider_id == model.provider_id
        && models[0].model_code == model.model_code && models[0].alias == model.display_name
        && db.get_assistant_prompt_params(assistant.id)?.is_empty()
        && configs.len() == CONFIG.len() && CONFIG.iter().all(|(name, value, kind)| configs.iter().any(|c|
            c.name == *name && c.value.as_deref() == Some(*value) && c.value_type == *kind && c.assistant_model_id == models[0].id)))
}

/// Stable IDs, no writes on unchanged loads, and no partially deleted profile on failure.
/// The caller serializes initialization; IMMEDIATE also serializes other SQLite writers
/// before reading a snapshot that will be changed (avoiding read-to-write lock upgrades).
pub(super) fn sync(db: &AssistantDatabase, model: &ButlerModelSelection, prompt: &str) -> Result<Assistant> {
    if let Some(assistant) = current(db)? {
        if matches(db, &assistant, model, prompt)? { return Ok(assistant); }
    }
    let tx = Transaction::new_unchecked(&db.conn, TransactionBehavior::Immediate)?;
    let id = if let Some(assistant) = current(db)? {
        if matches(db, &assistant, model, prompt)? { tx.commit()?; return Ok(assistant); }
        if assistant.name != BUTLER_SYSTEM_ASSISTANT_NAME || assistant.description.as_deref() != Some(BUTLER_SYSTEM_ASSISTANT_DESCRIPTION) {
            db.update_assistant(assistant.id, BUTLER_SYSTEM_ASSISTANT_NAME, BUTLER_SYSTEM_ASSISTANT_DESCRIPTION)?;
        }
        assistant.id
    } else {
        db.add_assistant(BUTLER_SYSTEM_ASSISTANT_NAME, BUTLER_SYSTEM_ASSISTANT_DESCRIPTION, Some(0), true)?
    };
    let mut prompts = db.get_assistant_prompt(id)?;
    prompts.sort_by_key(|p| p.id);
    for param in db.get_assistant_prompt_params(id)? {
        crate::db::cascade_delete::delete(&db.conn, "assistant_prompt_param", param.id)?;
    }
    if let Some(first) = prompts.first() {
        if first.prompt != prompt { db.update_assistant_prompt(first.id, prompt)?; }
        for extra in prompts.iter().skip(1) {
            crate::db::cascade_delete::delete(&db.conn, "assistant_prompt", extra.id)?;
        }
    } else { db.add_assistant_prompt(id, prompt)?; }

    let mut models = db.get_assistant_model(id)?;
    models.sort_by_key(|m| m.id);
    let model_id = if let Some(first) = models.first() {
        if first.provider_id != model.provider_id || first.model_code != model.model_code || first.alias != model.display_name {
            db.update_assistant_model(first.id, model.provider_id, &model.model_code, &model.display_name)?;
        }
        for extra in models.iter().skip(1) {
            crate::db::cascade_delete::delete(&db.conn, "assistant_model", extra.id)?;
        }
        first.id
    } else { db.add_assistant_model(id, model.provider_id, &model.model_code, &model.display_name)? };
    let configs = db.get_assistant_model_configs(id)?;
    for extra in configs.iter().filter(|c| !CONFIG.iter().any(|(name, _, _)| c.name == *name)) {
        crate::db::cascade_delete::delete(&db.conn, "assistant_model_config", extra.id)?;
    }
    for (name, value, kind) in CONFIG {
        if let Some(existing) = configs.iter().find(|c| c.name == name) {
            if existing.assistant_model_id != model_id || existing.value.as_deref() != Some(value) || existing.value_type != kind {
                db.conn.execute("UPDATE assistant_model_config SET assistant_model_id=?1,value=?2,value_type=?3 WHERE id=?4",
                    params![model_id,value,kind,existing.id])?;
            }
        } else { db.add_assistant_model_config(id, model_id, name, value, kind)?; }
    }
    let assistant = db.get_assistant(id)?;
    tx.commit()?;
    Ok(assistant)
}

#[cfg(test)]
#[path = "butler_assistant_profile_tests.rs"]
mod tests;
