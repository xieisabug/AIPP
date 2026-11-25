use super::*;

#[derive(Default)]
pub(super) struct BackgroundTasks {
    started: HashMap<String, i64>,
    roster: HashMap<String, i64>,
}

impl BackgroundTasks {
    /// Capture the launching response before a later user turn can replace it.
    /// A completion returns that owner so the following idle wake can link back.
    pub(super) fn observe(&mut self, frame: &Value, response: i64) -> Option<i64> {
        if frame["type"] != "system" { return None; }
        match frame["subtype"].as_str() {
            Some("background_tasks_changed") => {
                if let Some(tasks) = frame["tasks"].as_array() {
                    self.roster = tasks.iter().filter_map(|task| {
                        let id = task["task_id"].as_str()?;
                        let owner = self.started.get(id).or_else(|| self.roster.get(id)).copied().unwrap_or(response);
                        Some((id.to_owned(), owner))
                    }).collect();
                }
            }
            Some("task_started" | "task_progress") if frame["is_backgrounded"] != false => {
                if let Some(id) = frame["task_id"].as_str() {
                    self.started.entry(id.to_owned()).or_insert_with(|| self.roster.get(id).copied().unwrap_or(response));
                }
            }
            Some("task_notification") => {
                if let Some(id) = frame["task_id"].as_str() {
                    let owner = self.started.remove(id);
                    return self.roster.remove(id).or(owner);
                }
            }
            _ => {}
        }
        None
    }

    pub(super) fn pending(&self) -> bool { !self.started.is_empty() || !self.roster.is_empty() }
}

/// Claude can emit a zero-turn acknowledgement before the actual wake turn.
pub(super) fn lifecycle_result(frame: &Value) -> bool {
    frame["type"] == "result" && frame["origin"]["kind"] == "task-notification"
        && frame["subtype"] == "success" && frame["is_error"] != true
        && frame["num_turns"] == 0 && frame["result"] == ""
}

pub(super) fn starts_wake(frame: &Value) -> bool {
    if !frame["parent_tool_use_id"].is_null() || lifecycle_result(frame) { return false; }
    match frame["type"].as_str() {
        Some("system") => frame["subtype"] == "init",
        Some("assistant" | "stream_event") => true,
        Some("result") => frame["origin"]["kind"] == "task-notification",
        _ => false,
    }
}

pub(super) enum IdleEvent {
    Command(SessionCommand),
    Frame(Value),
}

pub(super) async fn read_idle_event(
    lines: &mut tokio::io::Lines<impl tokio::io::AsyncBufRead + Unpin>,
    commands: &mut mpsc::UnboundedReceiver<SessionCommand>,
) -> Result<Option<IdleEvent>, String> {
    loop {
        tokio::select! {
            biased;
            line = lines.next_line() => {
                let line = line.map_err(|e| format!("读取 Claude Code 后台输出失败：{e}"))?
                    .ok_or("Claude Code 在等待后台事件时关闭了输出")?;
                if let Ok(frame) = serde_json::from_str(&line) { return Ok(Some(IdleEvent::Frame(frame))); }
            }
            command = commands.recv() => return Ok(command.map(IdleEvent::Command)),
        }
    }
}

pub(super) fn can_continue(app: &tauri::AppHandle, conversation: i64, previous: i64) -> Result<bool, String> {
    let kind = crate::mcp::butler_tools::conversation_kind(app, Some(conversation)).map_err(|e| e.to_string())?;
    let interactive = match kind.as_str() {
        "normal" | "butler_contact" => true,
        "butler_main" => crate::api::butler_api::active_butler_main_conversation_id(app)? == Some(conversation),
        // Managed runs have their own completion/stop and delivery contract.
        _ => false,
    };
    Ok(interactive && crate::api::ai::events::output_goal_current(app, conversation, Some(previous)))
}

pub(super) async fn set_pending(app: &tauri::AppHandle, conversation: i64, run: &str, pending: bool) {
    let state = app.state::<crate::ClaudeSessionState>();
    let mut sessions = state.sessions.lock().await;
    if let Some(entry) = sessions.get_mut(&conversation).filter(|entry| entry.run_id == run) {
        entry.background_pending = pending;
        entry.touch();
    }
}

pub(super) fn create_reply(app: &tauri::AppHandle, identity: &AgentEventIdentity, previous: i64) -> Result<i64, AppError> {
    let db = ConversationDatabase::new(app)?;
    let repo = db.message_repo()?;
    let mut created = None;
    super::super::agent_driver::lifecycle::publish_if_current_binding(app, identity.conversation_id,
        CLAUDE_SDK_API_TYPE, identity.run_id.as_deref(), || {
            created = Some(insert_reply(&repo, identity, previous));
        })?;
    let id = created.ok_or_else(|| AppError::ProviderError("Claude 后台任务所属的会话进程已更换".into()))??;
    if let Err(error) = emit_to_chat_windows(app, identity.clone().with_response_message(id), ConversationEvent {
        r#type: "message_add".into(), data: json!({"message_id": id, "message_type": "response"}),
    }) {
        tracing::warn!(conversation_id = identity.conversation_id, message_id = id, %error, "Claude wake row saved but window notification failed");
    }
    Ok(id)
}

fn insert_reply(repo: &crate::db::conversation_db::MessageRepository, identity: &AgentEventIdentity, previous: i64) -> Result<i64, AppError> {
    let mut message = super::super::agent_driver::response_message(identity.conversation_id,
        super::super::agent_driver::session::AgentSessionKind::Claude);
    message.metadata_json = Some(json!({"origin": "claude_background_wake"}).to_string());
    Ok(repo.create_native_continuation(message, previous)?.id)
}

#[cfg(test)]
#[path = "claude_background_tests.rs"]
mod tests;
