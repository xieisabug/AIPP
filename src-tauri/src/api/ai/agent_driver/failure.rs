//! Shared failure normalization, persistence and terminal publication.
//! Native protocol errors remain verbatim causes; no provider fallback occurs.
use super::events::{AgentEventEnvelope, AgentEventIdentity, AgentEventSink, WindowEventSink, publish_event};
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::db::conversation_db::{ConversationDatabase, MessageRepository, Repository};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum AgentFailureStage {
    Configuration, Initialize, Resume, SendPrompt, Stream, Permission, Mcp,
    Cancel, Shutdown, Protocol, Process, Persistence, NativeSession,
}

impl AgentFailureStage {
    fn label(self) -> &'static str {
        match self {
            Self::Configuration => "configuration", Self::Initialize => "initialize",
            Self::Resume => "resume", Self::SendPrompt => "send_prompt", Self::Stream => "stream",
            Self::Permission => "permission", Self::Mcp => "mcp", Self::Cancel => "cancel",
            Self::Shutdown => "shutdown", Self::Protocol => "protocol", Self::Process => "process",
            Self::Persistence => "persistence", Self::NativeSession => "native_session",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentProcessDiagnostics {
    pub stderr: Option<String>,
    pub exit_status: Option<String>,
    pub protocol_error: Option<String>,
    pub unavailable_reason: Option<String>,
}

impl Default for AgentProcessDiagnostics {
    fn default() -> Self {
        Self { stderr: None, exit_status: None, protocol_error: None,
            unavailable_reason: Some("The native adapter did not supply separate stderr, exit-status or protocol fields; its complete original diagnostic is retained in cause".into()) }
    }
}

/// Retain diagnostic context while removing configured credentials and common
/// credential-shaped log fields before storing or publishing native stderr.
pub(crate) fn redact_agent_diagnostic(text: &str, secrets: &[String]) -> String {
    static FIELDS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    static TOKENS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let mut redacted = text.to_string();
    for secret in secrets { if !secret.is_empty() { redacted = redacted.replace(secret, "[redacted]"); } }
    let fields = FIELDS.get_or_init(|| regex::Regex::new(r#"(?i)((?:authorization|api[_-]?key|access[_-]?token|password|secret|token)[\"']?\s*[:=]\s*[\"']?)(?:bearer\s+)?[^\s,;\"'}]+"#).expect("static credential-field expression"));
    redacted = fields.replace_all(&redacted, "${1}[redacted]").into_owned();
    let tokens = TOKENS.get_or_init(|| regex::Regex::new(r"(?i)\bbearer\s+[^\s,;]+|\bsk-[A-Za-z0-9_-]{8,}").expect("static credential-token expression"));
    tokens.replace_all(&redacted, "[redacted]").into_owned()
}

/// Per-process diagnostic retention only, never a process or session owner.
#[derive(Clone, Default)]
pub(crate) struct AgentDiagnosticBuffer {
    lines: std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    secrets: std::sync::Arc<Vec<String>>,
}

impl AgentDiagnosticBuffer {
    pub(crate) fn for_environment(environment: &std::collections::HashMap<String, String>) -> Self {
        let mut secrets = environment.iter().filter_map(|(name, value)| {
            let name = name.to_ascii_lowercase();
            (!value.is_empty() && ["key", "token", "secret", "password", "credential"].iter().any(|part| name.contains(part)))
                .then(|| value.clone())
        }).collect::<Vec<_>>();
        secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        Self { secrets: std::sync::Arc::new(secrets), ..Default::default() }
    }

    pub(crate) fn sanitize(&self, text: &str) -> String {
        redact_agent_diagnostic(text, &self.secrets)
    }

    pub(crate) fn push(&self, line: impl Into<String>) {
        let line = self.sanitize(&line.into());
        let mut lines = self.lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if lines.len() >= 128 { lines.pop_front(); }
        let retained: String = line.chars().take(8192).collect();
        lines.push_back(if retained.len() < line.len() { format!("{retained} [diagnostic line truncated]") } else { retained });
    }

    pub(crate) fn diagnostics(&self, exit_status: Option<String>, protocol_error: Option<String>) -> AgentProcessDiagnostics {
        let stderr = self.lines.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().cloned().collect::<Vec<_>>().join("\n");
        let mut missing = Vec::new();
        if stderr.is_empty() { missing.push("stderr has not provided any diagnostic lines at failure publication"); }
        if exit_status.is_none() { missing.push("process exit status is not available at failure publication (the process may still be running)"); }
        AgentProcessDiagnostics { stderr: (!stderr.is_empty()).then_some(stderr), exit_status, protocol_error,
            unavailable_reason: (!missing.is_empty()).then(|| missing.join("; ")) }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct AgentFailure {
    pub identity: AgentEventIdentity,
    pub stage: AgentFailureStage,
    pub cause: String,
    pub process: AgentProcessDiagnostics,
    pub persistence_error: Option<String>,
}

impl AgentFailure {
    pub(crate) fn new(identity: AgentEventIdentity, stage: AgentFailureStage, cause: impl Into<String>) -> Self {
        let cause = redact_agent_diagnostic(&cause.into(), &[]);
        Self { identity, stage,
            cause: if cause.trim().is_empty() {
                "The native adapter returned an empty error; the underlying cause was not provided by the protocol/process layer".into()
            } else { cause },
            process: AgentProcessDiagnostics::default(), persistence_error: None }
    }

    pub(crate) fn with_process_diagnostics(mut self, process: AgentProcessDiagnostics) -> Self {
        self.process = process; self
    }

    pub(crate) fn content(&self, provider: &str) -> String {
        let mut context = vec![format!("stage={}", self.stage.label()), format!("driver_id={}", self.identity.driver_id),
            format!("conversation_id={}", self.identity.conversation_id)];
        for (key, value) in [
            ("instance_id", self.identity.instance_id.as_deref()),
            ("session_id", self.identity.native_session_id.as_deref()),
            ("run_id", self.identity.run_id.as_deref()), ("turn_id", self.identity.turn_id.as_deref()),
        ] {
            if let Some(value) = value { context.push(format!("{key}={value}")); }
        }
        if let Some(message_id) = self.identity.response_message_id { context.push(format!("message_id={message_id}")); }
        let mut content = format!("{provider} 运行失败 [{}]：{}", context.join(", "), self.cause);
        for (key, value) in [
            ("stderr", self.process.stderr.as_deref()), ("exit_status", self.process.exit_status.as_deref()),
            ("protocol_error", self.process.protocol_error.as_deref()),
            ("diagnostics_unavailable", self.process.unavailable_reason.as_deref()),
            ("failure_persistence_error", self.persistence_error.as_deref()),
        ] {
            if let Some(value) = value { content.push_str(&format!("\n{key}: {value}")); }
        }
        content
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FailurePublication {
    Published,
    /// The existing message terminal marker prevents a duplicate failure or a
    /// late old run from overwriting a completed message. No global event filter.
    AlreadyFinishedOrMissing,
}

fn publish_failure_events(sink: &impl AgentEventSink, failure: &AgentFailure, provider: &str) -> Result<(), String> {
    let message_id = failure.identity.response_message_id.ok_or("Agent failure publication requires response_message_id")?;
    let content = failure.content(provider);
    let mut events = failure_events(failure.identity.conversation_id, message_id, content, &failure.cause);
    // Existing frontend keys are unchanged; structured diagnostics are additive.
    events[1].data["agent_failure"] = serde_json::to_value(failure).map_err(|error| error.to_string())?;
    let mut errors = Vec::new();
    for event in events {
        if let Err(error) = publish_event(sink, &AgentEventEnvelope::from_conversation_event(failure.identity.clone(), event)) {
            errors.push(error);
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors.join("; ")) }
}

pub(crate) fn persist_and_publish_failure(
    repo: &MessageRepository, sink: &impl AgentEventSink, failure: &AgentFailure, provider: &str,
) -> Result<FailurePublication, String> {
    let message_id = failure.identity.response_message_id.ok_or("Agent failure persistence requires response_message_id")?;
    let details = serde_json::to_string(failure).map_err(|error| error.to_string())?;
    let persisted = repo.finish_agent_failure(
        failure.identity.conversation_id, message_id, &failure.content(provider), &details,
    );
    match persisted {
        Ok(false) => Ok(FailurePublication::AlreadyFinishedOrMissing),
        Ok(true) => {
            publish_failure_events(sink, failure, provider)?;
            Ok(FailurePublication::Published)
        }
        Err(error) => {
            let mut failure = failure.clone();
            failure.persistence_error = Some(error.to_string());
            let publication = publish_failure_events(sink, &failure, provider);
            Err(format!("Agent failure persistence failed: {error}; publication={publication:?}"))
        }
    }
}

pub(crate) fn emit_failure_to_window(
    app_handle: &tauri::AppHandle, identity: AgentEventIdentity, message_id: i64,
    window: &tauri::Window, provider: &str, stage: AgentFailureStage, cause: &str,
) -> Result<FailurePublication, String> {
    emit_normalized_failure_to_window(app_handle, window,
        AgentFailure::new(identity.with_response_message(message_id), stage, cause), provider)
}

pub(crate) fn emit_normalized_failure_to_window(
    app_handle: &tauri::AppHandle, window: &tauri::Window, failure: AgentFailure, provider: &str,
) -> Result<FailurePublication, String> {
    emit_normalized_failure_to_sink(app_handle, &WindowEventSink(window), failure, provider)
}

pub(crate) fn emit_normalized_failure_to_sink(
    app_handle: &tauri::AppHandle, sink: &impl AgentEventSink, mut failure: AgentFailure, provider: &str,
) -> Result<FailurePublication, String> {
    let repo = ConversationDatabase::new(app_handle).map_err(|error| error.to_string())
        .and_then(|db| db.message_repo().map_err(|error| error.to_string()));
    match repo {
        Ok(repo) => persist_and_publish_failure(&repo, sink, &failure, provider),
        Err(error) => {
            failure.persistence_error = Some(error.to_string());
            let publication = publish_failure_events(sink, &failure, provider);
            Err(format!("Agent failure database unavailable: {error}; publication={publication:?}"))
        }
    }
}

// Retained for callers testing the existing compatibility wire contract.
pub(super) fn persist_failure_message(repo: &MessageRepository, message_id: i64, content: &str) -> rusqlite::Result<()> {
    if let Some(message) = repo.read(message_id)? {
        repo.finish_agent_failure(message.conversation_id, message_id, content, "{}")?;
    }
    Ok(())
}

pub(super) fn failure_events(conversation_id: i64, message_id: i64, content: String, error: &str) -> [ConversationEvent; 2] {
    [
        ConversationEvent {
            r#type: "message_update".into(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id, message_type: "error".into(), content, is_done: true,
                token_count: Some(0), input_token_count: Some(0), output_token_count: Some(0),
                ttft_ms: None, tps: None,
            }).unwrap(),
        },
        ConversationEvent {
            r#type: "stream_complete".into(),
            data: json!({ "conversation_id": conversation_id, "response_message_id": message_id,
                "has_response": false, "error": error }),
        },
    ]
}

#[cfg(test)]
#[path = "failure_tests.rs"]
mod tests;
