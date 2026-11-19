use super::*;
use crate::api::ai::agent_driver::{lifecycle, registry::AgentSessionLookup};
use std::sync::{LazyLock, Mutex};
use tokio_util::sync::CancellationToken;

pub(super) struct SteerTarget {
    pub session: AgentSessionLookup,
    pub turn: String,
    pub assistant_id: i64,
}
tokio::task_local! { pub(super) static STEER_TARGET: Option<SteerTarget>; }

struct Handoff {
    queue_id: i64,
    stop: CancellationToken,
    run: String,
    terminal: Option<tokio::sync::oneshot::Sender<()>>,
}
static HANDOFFS: LazyLock<Mutex<HashMap<i64, Handoff>>> = LazyLock::new(Default::default);

pub(super) fn stop(conversation: i64) {
    if let Some(handoff) = HANDOFFS.lock().unwrap().get(&conversation) {
        handoff.stop.cancel();
    }
}

/// An explicit handoff consumes only the terminal event from the original actor.
pub(crate) async fn finish_turn(app: &tauri::AppHandle, window: &tauri::Window, conversation: i64, run: &str, success: bool) {
    let handoff = {
        let mut pending = HANDOFFS.lock().unwrap();
        pending.get_mut(&conversation).filter(|entry| entry.run == run).and_then(|entry| entry.terminal.take())
    };
    if let Some(terminal) = handoff {
        let _ = terminal.send(());
    } else if success {
        try_dispatch_queued_message_after_completion(app, window, conversation).await;
    }
}

fn active(session: &AgentSessionLookup) -> Result<bool, AppError> {
    session.snapshot.get("has_active_prompt").or_else(|| session.snapshot.get("is_busy"))
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| AppError::ProviderError(format!("{} session {} omitted active-turn state", session.handle.metadata().id, session.run_id)))
}

async fn wait_for_interrupted_turn(
    cancel: impl std::future::Future<Output = Result<(), AppError>>,
    mut settled: tokio::sync::oneshot::Receiver<()>,
    stop: &CancellationToken,
    driver: &str,
    run: &str,
) -> Result<(), AppError> {
    let receipt_lost = |_| AppError::ProviderError(format!("{driver} 回合结束回执丢失 [run={run}]"));
    let wait = async {
        tokio::select! {
            biased;
            terminal = &mut settled => terminal.map_err(receipt_lost),
            result = cancel => {
                result?;
                settled.await.map_err(receipt_lost)
            }
        }
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Err(AppError::UnknownError("用户已停止，插队消息已暂停".into())),
        result = tokio::time::timeout(std::time::Duration::from_secs(40), wait) =>
            result.unwrap_or_else(|_| Err(AppError::ProviderError(format!("{driver} 等待回合中断超时 [run={run}]")))),
    }
}

pub(super) async fn interrupt(app: &tauri::AppHandle, window: &tauri::Window, row: &QueuedConversationMessage) -> Result<(), AppError> {
    let conversation = row.conversation_id;
    let Some(session) = lifecycle::lookup_session(app, conversation).await? else { return Ok(()); };
    if !active(&session)? { return Ok(()); }
    let driver = session.handle.metadata().id;
    let db = ConversationDatabase::new(app)?;
    let owner = db.conversation_repo()?.read(conversation)?.and_then(|c| c.assistant_id);
    let steer = matches!(driver, "pi" | "codex_app_server") && owner == Some(row.assistant_id);
    let stop = CancellationToken::new();
    let (terminal, settled) = tokio::sync::oneshot::channel();
    if !steer {
        let mut pending = HANDOFFS.lock().unwrap();
        if pending.contains_key(&conversation) { return Ok(()); }
        pending.insert(conversation, Handoff { queue_id: row.id, stop: stop.clone(), run: session.run_id.clone(), terminal: Some(terminal) });
    }
    let (app, window, row) = (app.clone(), window.clone(), row.clone());
    tokio::spawn(async move {
        let result = async {
            let request: AiRequest = serde_json::from_str(&row.request_json).map_err(|error| AppError::ParseError(error.to_string()))?;
            let conn = db.get_connection()?;
            crate::db::butler_control::ensure_turn(&conn, conversation).map_err(|e| AppError::UnknownError(e.to_string()))?;
            if let Some(revision) = request.butler_goal_revision {
                crate::db::butler_goal::assert_current(&conn, conversation, revision).map_err(|e| AppError::UnknownError(e.to_string()))?;
            }
            drop(conn);
            let Some(claimed) = db.queued_message_repo()?.take_for_dispatch_by_id(row.id)? else { return Ok(()); };
            if steer {
                let turn = session.snapshot["current_turn_id"].as_str()
                    .ok_or_else(|| AppError::ProviderError(format!("{driver} 未提供当前回合 ID，无法插队")))?;
                let target = SteerTarget { turn: turn.to_string(), assistant_id: row.assistant_id, session };
                dispatch_queued_target(app.clone(), window.clone(), claimed, Some(target)).map(|_| ())
            } else {
                wait_for_interrupted_turn(session.handle.cancel_current_prompt(), settled, &stop, driver, &session.run_id).await?;
                let binding = lifecycle::session_binding(&app, conversation)?;
                if binding.as_ref().map(|b| b.run_id.as_str()) != Some(session.run_id.as_str()) {
                    return Err(AppError::ProviderError(format!("{driver} 插队前会话已切换 [run={}]", session.run_id)));
                }
                dispatch_queued_message(app.clone(), window.clone(), claimed).map(|_| ())
            }
        }.await;
        {
            let mut pending = HANDOFFS.lock().unwrap();
            if pending.get(&conversation).is_some_and(|p| p.queue_id == row.id) { pending.remove(&conversation); }
        }
        if let Err(error) = result {
            match db.queued_message_repo().and_then(|repo| Ok(repo.pause_dispatch(row.id)?)) {
                Ok(Some(paused)) => emit_queued_message_payload(&app, "queued_message_update", &paused),
                Ok(None) => {}
                Err(persist) => tracing::error!(queue_id=row.id, %persist, "Failed to pause interrupted queue entry"),
            }
            record_request_error(&app, conversation, &format!("插队失败 [queue={}]: {error}；原输入已保留。", row.id));
        }
    });
    Ok(())
}

#[cfg(test)]
#[path = "native_queue_tests.rs"]
mod tests;
