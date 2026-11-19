use super::*;
use rusqlite::{params, OptionalExtension};
use crate::db::conversation_db::Conversation;

#[tauri::command]
pub async fn run_conversation_context_command(
    app_handle: tauri::AppHandle, conversation_id: i64, command: String,
) -> Result<(), String> {
    run(&app_handle, conversation_id, &command).await.map_err(|error| format!("会话 {conversation_id} /{command}：{error}"))
}

async fn run(app_handle: &tauri::AppHandle, conversation_id: i64, command: &str) -> Result<(), AppError> {
    let _guard = crate::api::butler_peer::admit_turn(conversation_id).await;
    if crate::api::butler_api::is_butler_main_running(&app_handle, conversation_id).await {
        return Err(AppError::UnknownError(format!("会话 {conversation_id} 正在回复，请结束后重试")));
    }
    let db = ConversationDatabase::new(&app_handle)?;
    let conversation = db.conversation_repo()?.read(conversation_id)?
        .ok_or_else(|| AppError::UnknownError(format!("会话 {conversation_id} 不存在")))?;
    let assistant_id = conversation.assistant_id.ok_or_else(|| AppError::UnknownError(format!("会话 {conversation_id} 没有绑定助手")))?;
    let assistant = current_assistant(app_handle, &conversation, assistant_id).await?;
    let butler = crate::mcp::builtin_mcp::templates::is_butler_conversation_kind(&conversation.conversation_kind);
    match command {
        "reload" if butler => reload(app_handle, &db, &conversation, &assistant).await.map(|_| ()),
        "compact" if assistant.assistant.assistant_type == Some(4) => {
            let state = crate::api::ai::acp::get_acp_session_state(app_handle.clone(), conversation_id).await.map_err(AppError::UnknownError)?;
            let provider = crate::api::assistant_api::resolve_acp_provider_id(&assistant.model, &assistant.model_configs)
                .ok_or_else(|| AppError::ProviderError("Agent 未绑定提供商".into()))?;
            let llm = LLMDatabase::new(app_handle)?;
            let api_type = llm.get_llm_provider(provider)?.api_type;
            let supported = api_type == crate::api::ai::codex_app_server::CODEX_APP_SERVER_API_TYPE ||
                crate::api::ai::claude_sdk::is_claude_code_provider(&api_type, &llm.get_llm_provider_config(provider)?) ||
                state.as_ref().and_then(|v| v["available_commands"].as_array()).is_some_and(|items| items.iter()
                    .any(|item| item["name"].as_str().is_some_and(|s| s.trim_start_matches('/') == "compact")));
            if !supported { return Err(AppError::ProviderError(format!("Agent {api_type} 未提供原生压缩命令"))); }
            let role = if butler { instructions(app_handle, &conversation, &assistant).await?.1 } else { None };
            let features = app_handle.state::<FeatureConfigState>().config_feature_map.lock().await.clone();
            let window = crate::api::butler_api::resolve_or_create_butler_execution_window(app_handle).map_err(AppError::UnknownError)?;
            let result = crate::api::ai::agent_driver::send_turn(app_handle.clone(), window, crate::api::ai::agent_driver::AgentTurnRequest {
                assistant: &assistant, conversation_id, native_butler_role: role, source_message_id: None,
                prompt: "/compact".into(), display_prompt: "/compact".into(), override_model_id: None, overrides: &None,
                feature_configs: &features, messages: &[("user".into(), "/compact".into(), vec![])],
            }).await?;
            if let Some(error) = result.error_message { return Err(AppError::ProviderError(error)); }
            Ok(())
        }
        "compact" => {
            compact(&app_handle, &db, &conversation, &assistant).await?;
            if butler { reload(&app_handle, &db, &conversation, &assistant).await?; }
            Ok(())
        }
        _ => Err(AppError::UnknownError(format!("会话 {conversation_id} 不支持命令 /{command}"))),
    }
}

async fn current_assistant(app: &tauri::AppHandle, conversation: &Conversation, id: i64) -> Result<AssistantDetail, AppError> {
    if conversation.conversation_kind == "butler_main" {
        crate::api::butler_api::refresh_butler_system_assistant_if_ready(app).await.map_err(AppError::UnknownError)?;
    }
    get_assistant(app.clone(), id).map_err(AppError::UnknownError)
}

async fn instructions(app: &tauri::AppHandle, conversation: &Conversation, assistant: &AssistantDetail) -> Result<(String, Option<String>, bool), AppError> {
    let id = conversation.id;
    let prompt = assistant.prompts.first().ok_or_else(|| AppError::UnknownError(format!("会话 {id} 的助手没有系统提示词记录")))?;
    let engine = build_template_engine(app).map_err(|e| AppError::UnknownError(e.to_string()))?;
    let selected = app.state::<AppState>().selected_text.lock().await.clone();
    let vars = HashMap::from([("conversation_id".into(), id.to_string()), ("selected_text".into(), selected)]);
    let role = engine.parse(&prompt.prompt, &vars).await;
    let overrides = enforce_butler_mcp_override(&assistant.assistant.name, None);
    let mcp = collect_mcp_info_for_conversation(app, assistant.assistant.id, overrides.as_ref(), None, Some(id)).await?;
    let mcp = filter_butler_tools_for_kind(mcp, Some(&conversation.conversation_kind));
    compose_conversation_instructions(app, assistant, role, &mcp, Some(conversation), &id.to_string(), None).await
}

async fn reload(app: &tauri::AppHandle, db: &ConversationDatabase, conversation: &Conversation, assistant: &AssistantDetail) -> Result<String, AppError> {
    let id = conversation.id;
    let (system, _, _) = instructions(app, conversation, assistant).await?;
    let conn = db.get_connection()?;
    replace_system_head(&conn, id, &system)?;
    Ok(system)
}

fn replace_system_head(conn: &rusqlite::Connection, id: i64, system: &str) -> rusqlite::Result<()> {
    // The first system row is the role. Later system rows may be compaction checkpoints.
    conn.execute("UPDATE message SET content=?2,
        metadata_json=json_set(COALESCE(metadata_json,'{}'),'$.context_reload_after',
          (SELECT MAX(id) FROM message WHERE conversation_id=?1))
        WHERE id=(SELECT id FROM message WHERE conversation_id=?1 AND message_type='system'
          AND content NOT LIKE '<!-- compacted_%' ORDER BY created_time,id LIMIT 1)", params![id,system])?;
    Ok(())
}

async fn compact(app: &tauri::AppHandle, db: &ConversationDatabase, conversation: &Conversation, assistant: &AssistantDetail) -> Result<(), AppError> {
    let id = conversation.id;
    let rows = db.message_repo()?.list_by_conversation_id(id)?;
    let (messages, metadata) = build_message_list_with_metadata_from_db(&rows, BranchSelection::LatestBranch);
    if messages.len() <= 1 { return Err(AppError::UnknownError(format!("会话 {id} 暂无可压缩的消息"))); }
    let model = resolve_ask_model_detail(&LLMDatabase::new(app)?, None, assistant).map_err(AppError::UnknownError)?;
    let configs = app.state::<FeatureConfigState>().config_feature_map.lock().await.clone();
    let proxy = get_network_proxy_from_config(&configs);
    let provider = &model.provider.api_type;
    let prepared = crate::api::copilot_token_manager::prepare_provider_configs(app, provider, &model.configs, proxy.as_deref()).await.map_err(AppError::ProviderError)?;
    let proxy_enabled = model.configs.iter().any(|c| c.name == "proxy_enabled" && c.value == "true");
    let client = genai_client::create_client_with_config(Some((app, id)), &prepared, &model.model.code, provider, Some(&model.model.request_mode),
        proxy.as_deref(), proxy_enabled, Some(get_request_timeout_from_config(&configs)), false, &configs)?;
    let chief = is_butler_system_assistant_name(&assistant.assistant.name);
    let source_message_id = {
        let conn = db.get_connection()?;
        match crate::db::butler_goal::current_revision(&conn, id)? {
            Some(revision) => conn.query_row("SELECT MAX(message_id) FROM butler_goal_turn WHERE conversation_id=?1 AND revision=?2",
                params![id, revision], |row| row.get(0))?,
            None => None,
        }
    };
    let context = CompactionContext {
        source_message_id, cancel_token: None, client: &client, model_name: &model.model.code, model_id: Some(model.model.id),
        conversation_id: id, checkpoint_store: db, summary_role: context_summary_role(db, id, chief),
        require_complete_summary: chief || requires_complete_context_summary(db, id), message_ids: metadata.message_ids,
    };
    let budget = ContextBudget { enabled: true, compaction_threshold: 0.0, tail_ratio: 0.0, ..ContextBudget::from_config(&configs) };
    let result = context_manager::fit_to_budget_with_compaction(messages, &budget, &metadata.db_token_counts, context).await?;
    if !result.compacted { return Err(AppError::UnknownError(format!("会话 {id} 缺少可独立压缩的历史消息段"))); }
    Ok(())
}

pub(crate) fn save_assistant_prompts(
    app: &tauri::AppHandle,
    db: &crate::db::assistant_db::AssistantDatabase,
    assistant_id: i64,
    prompts: &[crate::db::assistant_db::AssistantPrompt],
) -> Result<(), AppError> {
    if prompts.is_empty() { return Ok(()); }
    let path = crate::db::get_db_path(app, "conversation.db").map_err(AppError::DatabaseError)?;
    db.conn.execute("ATTACH DATABASE ?1 AS chat", [path.to_string_lossy().as_ref()])?;
    let tx = db.conn.unchecked_transaction()?;
    let previous = db.get_assistant_prompt(assistant_id)?.into_iter().next().map(|p| p.prompt);
    for prompt in prompts {
        if prompt.id == 0 {
            db.add_assistant_prompt(prompt.assistant_id, &prompt.prompt)?;
        } else {
            db.update_assistant_prompt(prompt.id, &prompt.prompt)?;
        }
    }
    let current = db.get_assistant_prompt(assistant_id)?.into_iter().next().map(|p| p.prompt);
    let notice = match current {
        Some(prompt) if previous.as_ref() != Some(&prompt) => append_prompt_notice(&tx, assistant_id, &prompt)?,
        _ => None,
    };
    tx.commit()?;
    if let Some((conversation, message)) = notice {
        send_conversation_event_to_chat_windows(app, conversation, ConversationEvent {
            r#type: "message_add".into(), data: serde_json::json!({"message_id":message,"message_type":"user"}),
        });
    }
    Ok(())
}

fn append_prompt_notice(conn: &rusqlite::Connection, assistant: i64, prompt: &str) -> rusqlite::Result<Option<(i64,i64)>> {
    let initialized: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM chat.sqlite_master WHERE type='table' AND name='conversation')", [], |r| r.get(0))?;
    if !initialized { return Ok(None); }
    let contact: Option<i64> = conn.query_row("SELECT id FROM chat.conversation WHERE assistant_id=?1 AND conversation_kind='butler_contact'",
        [assistant], |row| row.get(0)).optional()?;
    let Some(id) = contact else { return Ok(None) };
    let content = if prompt.trim().is_empty() {
        "AIPP 助手指令更新：用户已清空助手指令，原有角色、回复风格和工作要求已撤销。".to_owned()
    } else {
        format!("AIPP 助手指令更新：以下是用户保存的完整助手指令，替换之前配置的角色、回复风格和工作要求，移除的要求已撤销。\n\n{prompt}")
    };
    let count = conn.execute("INSERT INTO chat.message(conversation_id,message_type,content,created_time,token_count,input_token_count,output_token_count,metadata_json)
        SELECT ?1,'user',?2,?3,0,0,0,json_object('origin','assistant_prompt_update')
        WHERE EXISTS(SELECT 1 FROM chat.message WHERE conversation_id=?1)
        AND ?2 IS NOT (SELECT content FROM chat.message WHERE conversation_id=?1 AND json_valid(metadata_json)
          AND json_extract(metadata_json,'$.origin')='assistant_prompt_update' ORDER BY id DESC LIMIT 1)",
        params![id,content,chrono::Utc::now()])?;
    let message = conn.last_insert_rowid();
    Ok((count == 1).then_some((id,message)))
}

pub(super) async fn refresh_after_compaction(app: &tauri::AppHandle, id: i64, result: &mut context_manager::FitResult) -> Result<(), AppError> {
    if !result.compacted { return Ok(()); }
    let db = ConversationDatabase::new(app)?;
    let Some(conversation) = db.conversation_repo()?.read(id)? else { return Err(AppError::UnknownError(format!("会话 {id} 已不存在"))); };
    if crate::mcp::builtin_mcp::templates::is_butler_conversation_kind(&conversation.conversation_kind) {
        let assistant_id = conversation.assistant_id.ok_or_else(|| AppError::UnknownError(format!("会话 {id} 未绑定助手")))?;
        let assistant = current_assistant(app, &conversation, assistant_id).await?;
        let system = reload(app, &db, &conversation, &assistant).await?;
        if let Some(head) = result.messages.first_mut().filter(|m| m.0 == "system") { head.1 = system; }
    }
    Ok(())
}
#[cfg(test)]
#[path = "context_commands_tests.rs"]
mod tests;
