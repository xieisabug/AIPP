//! ACP (Agent Client Protocol) integration module
//! Handles communication with ACP-compatible agents via stdio

use agent_client_protocol::{self as acp, Agent as _, Client as AcpClient, ClientSideConnection};
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent, MCPToolCallUpdateEvent};
use crate::db::assistant_db::AssistantModelConfig;
use crate::db::conversation_db::ConversationDatabase;
use crate::db::llm_db::LLMProviderConfig;
use crate::errors::AppError;
use crate::mcp::builtin_mcp::operation::{
    bash_ops::BashOperations,
    file_ops::FileOperations,
    permission::PermissionManager,
    state::OperationState,
    types::{
        BashProcessStatus, ExecuteBashRequest, GetBashOutputRequest, ReadFileRequest,
        WriteFileRequest,
    },
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tokio::process::Command;
use tokio::sync::Mutex as TokioMutex;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info};

/// ACP configuration extracted from assistant_model_config
#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub additional_args: Vec<String>,
}

/// Resolve ACP CLI command to its full path
/// 
/// This function tries to find the CLI executable in the following order:
/// 1. If the command is already an absolute path, use it directly
/// 2. Check ~/.bun/bin/ for bun-installed global packages
/// 3. Check system PATH
/// 4. Fall back to the original command (let the system handle it)
fn resolve_acp_cli_path(cli_command: &str) -> PathBuf {
    let cli_path = PathBuf::from(cli_command);
    
    // If it's already an absolute path, use it directly
    if cli_path.is_absolute() {
        info!("ACP: CLI command is already an absolute path");
        return cli_path;
    }
    
    // Check ~/.bun/bin/ first (bun-installed global packages)
    if let Some(home) = dirs::home_dir() {
        let bun_bin_path = home.join(".bun").join("bin").join(cli_command);
        info!("ACP: Checking bun bin path: {}", bun_bin_path.display());
        if bun_bin_path.exists() {
            info!("ACP: Found CLI in bun bin: {}", bun_bin_path.display());
            return bun_bin_path;
        }
    }
    
    // Check system PATH using `which` command
    if let Ok(output) = std::process::Command::new("which")
        .arg(cli_command)
        .output()
    {
        if output.status.success() {
            let path_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path_str.is_empty() {
                info!("ACP: Found CLI in system PATH: {}", path_str);
                return PathBuf::from(path_str);
            }
        }
    }
    
    info!("ACP: CLI not found in known paths, using original command: {}", cli_command);
    cli_path
}

/// Terminal ID to bash_id mapping for ACP terminal management
struct TerminalMapping {
    terminal_id: acp::TerminalId,
    bash_id: String,
}

/// Extract text content from a ContentBlock
fn extract_content_text(content: &acp::ContentBlock) -> String {
    match content {
        acp::ContentBlock::Text(text_content) => text_content.text.clone(),
        acp::ContentBlock::Image(_) => "[Image]".to_string(),
        acp::ContentBlock::Audio(_) => "[Audio]".to_string(),
        acp::ContentBlock::ResourceLink(resource_link) => resource_link.uri.clone(),
        acp::ContentBlock::Resource(resource) => {
            // Extract URI from the nested resource enum
            match &resource.resource {
                acp::EmbeddedResourceResource::TextResourceContents(text) => text.uri.clone(),
                acp::EmbeddedResourceResource::BlobResourceContents(blob) => blob.uri.clone(),
                _ => "[Resource]".to_string(),
            }
        }
        _ => "[Unknown content]".to_string(),
    }
}

/// Convert ACP ToolCallStatus to string for frontend
fn tool_status_to_string(status: acp::ToolCallStatus) -> String {
    match status {
        acp::ToolCallStatus::Pending => "pending".to_string(),
        acp::ToolCallStatus::InProgress => "executing".to_string(),
        acp::ToolCallStatus::Completed => "success".to_string(),
        acp::ToolCallStatus::Failed => "failed".to_string(),
        _ => "unknown".to_string(),
    }
}

/// Convert ACP ToolCallId to i64 for frontend
fn tool_call_id_to_i64(id: &acp::ToolCallId) -> i64 {
    id.0.parse().unwrap_or_else(|_| {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        id.0.hash(&mut hasher);
        hasher.finish() as i64
    })
}

/// Tauri client implementation that forwards ACP events to the frontend
pub struct AcpTauriClient {
    pub app_handle: tauri::AppHandle,
    pub conversation_id: i64,
    pub message_id: i64,
    pub window: tauri::Window,
    operation_state: Arc<OperationState>,
    permission_manager: Arc<PermissionManager>,
    /// Accumulated response content buffer for database persistence
    response_content_buffer: Arc<TokioMutex<String>>,
    /// Accumulated reasoning content buffer for database persistence
    reasoning_content_buffer: Arc<TokioMutex<String>>,
}

impl AcpTauriClient {
    pub fn new(
        app_handle: tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
        window: tauri::Window,
        operation_state: Arc<OperationState>,
        permission_manager: Arc<PermissionManager>,
    ) -> Self {
        Self {
            app_handle,
            conversation_id,
            message_id,
            window,
            operation_state,
            permission_manager,
            response_content_buffer: Arc::new(TokioMutex::new(String::new())),
            reasoning_content_buffer: Arc::new(TokioMutex::new(String::new())),
        }
    }

    /// Convert ACP session_id to conversation_id
    fn get_conversation_id(&self) -> Option<i64> {
        Some(self.conversation_id)
    }

    /// Update message content in database
    fn update_message_in_db(&self, content: &str) {
        if let Ok(db) = ConversationDatabase::new(&self.app_handle) {
            if let Ok(repo) = db.message_repo() {
                if let Err(e) = repo.update_content(self.message_id, content) {
                    error!("ACP: Failed to update message in DB: {}", e);
                }
            }
        }
    }

    /// Get the accumulated response content
    pub async fn get_response_content(&self) -> String {
        self.response_content_buffer.lock().await.clone()
    }

    /// Get the accumulated reasoning content
    pub async fn get_reasoning_content(&self) -> String {
        self.reasoning_content_buffer.lock().await.clone()
    }

    /// Send completion event to frontend
    pub fn send_done_event(&self, message_type: &str, content: &str) {
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id: self.message_id,
                message_type: message_type.to_string(),
                content: content.to_string(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            }).unwrap(),
        };

        let event_name = format!("conversation_event_{}", self.conversation_id);
        if let Err(e) = self.window.emit(&event_name, event) {
            error!("ACP: Failed to emit done event: {}", e);
        }
    }

    /// Send error event to frontend
    pub fn send_error_event(&self, error_message: &str) {
        // Update database with error message
        self.update_message_in_db(error_message);

        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id: self.message_id,
                message_type: "error".to_string(),
                content: error_message.to_string(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            }).unwrap(),
        };

        let event_name = format!("conversation_event_{}", self.conversation_id);
        if let Err(e) = self.window.emit(&event_name, event) {
            error!("ACP: Failed to emit error event: {}", e);
        }
    }
}

#[async_trait::async_trait(?Send)]
impl AcpClient for AcpTauriClient {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<(), acp::Error> {
        // Log the notification type for debugging
        let update_type = std::format!("{:?}", args.update)
            .split('(')
            .next()
            .unwrap_or("Unknown")
            .to_string();
        debug!("ACP session_notification: type={}, message_id={}", update_type, self.message_id);

        match args.update {
            // User message streaming - just log, don't emit to UI (user message is already shown)
            acp::SessionUpdate::UserMessageChunk(acp::ContentChunk { content, .. }) => {
                let text = extract_content_text(&content);
                debug!("ACP UserMessageChunk (ignored): {}", text);
                // Note: We intentionally don't emit this to UI because:
                // 1. The user message is already displayed in the conversation
                // 2. Writing to self.message_id (which is the response message) would be wrong
            }

            // Agent response streaming - accumulate, persist to DB, and emit to frontend
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk { content, .. }) => {
                let text = extract_content_text(&content);
                info!("ACP AgentMessageChunk: {} chars", text.len());

                // Accumulate content
                let full_content = {
                    let mut buffer = self.response_content_buffer.lock().await;
                    buffer.push_str(&text);
                    buffer.clone()
                };

                // Persist to database
                self.update_message_in_db(&full_content);

                // Emit full content to frontend (matching existing UI behavior)
                let event = ConversationEvent {
                    r#type: "message_update".to_string(),
                    data: serde_json::to_value(MessageUpdateEvent {
                        message_id: self.message_id,
                        message_type: "response".to_string(),
                        content: full_content,
                        is_done: false,
                        token_count: None,
                        input_token_count: None,
                        output_token_count: None,
                        ttft_ms: None,
                        tps: None,
                    }).unwrap(),
                };

                let event_name = format!("conversation_event_{}", self.conversation_id);
                match self.window.emit(&event_name, event) {
                    Ok(_) => debug!("ACP: Emitted AgentMessageChunk event"),
                    Err(e) => error!("ACP: Failed to emit AgentMessageChunk event: {}", e),
                }
            }

            // Agent internal reasoning (thoughts) - accumulate and emit as reasoning message type
            acp::SessionUpdate::AgentThoughtChunk(acp::ContentChunk { content, .. }) => {
                let text = extract_content_text(&content);
                info!("ACP AgentThoughtChunk: {} chars", text.len());

                // Accumulate reasoning content
                let full_reasoning = {
                    let mut buffer = self.reasoning_content_buffer.lock().await;
                    buffer.push_str(&text);
                    buffer.clone()
                };

                // Emit full reasoning content to frontend
                let event = ConversationEvent {
                    r#type: "message_update".to_string(),
                    data: serde_json::to_value(MessageUpdateEvent {
                        message_id: self.message_id,
                        message_type: "reasoning".to_string(),
                        content: full_reasoning,
                        is_done: false,
                        token_count: None,
                        input_token_count: None,
                        output_token_count: None,
                        ttft_ms: None,
                        tps: None,
                    }).unwrap(),
                };

                let _ = self.window.emit(
                    format!("conversation_event_{}", self.conversation_id).as_str(),
                    event
                );
            }

            // New tool call initiated - emit as MCP tool call update with pending status
            acp::SessionUpdate::ToolCall(tool_call) => {
                info!("ACP ToolCall: id={:?}, title={:?}", tool_call.tool_call_id, tool_call.title);

                let call_id = tool_call_id_to_i64(&tool_call.tool_call_id);

                let event = ConversationEvent {
                    r#type: "mcp_tool_call_update".to_string(),
                    data: serde_json::to_value(MCPToolCallUpdateEvent {
                        call_id,
                        conversation_id: self.conversation_id,
                        status: "pending".to_string(),
                        result: None,
                        error: None,
                        started_time: Some(chrono::Utc::now()),
                        finished_time: None,
                    }).unwrap(),
                };

                let _ = self.window.emit(
                    format!("conversation_event_{}", self.conversation_id).as_str(),
                    event
                );
            }

            // Tool call status update - emit as MCP tool call update
            acp::SessionUpdate::ToolCallUpdate(update) => {
                info!("ACP ToolCallUpdate: id={:?}, status={:?}", update.tool_call_id, update.fields.status);

                let call_id = tool_call_id_to_i64(&update.tool_call_id);

                let status_str = update.fields.status.as_ref()
                    .map(|s| tool_status_to_string(s.clone()))
                    .unwrap_or_else(|| "unknown".to_string());

                let (finished_time, result, error) = match &update.fields.status {
                    Some(acp::ToolCallStatus::Completed) => {
                        (Some(chrono::Utc::now()), update.fields.raw_output.as_ref(), None)
                    }
                    Some(acp::ToolCallStatus::Failed) => {
                        (Some(chrono::Utc::now()), None, update.fields.raw_output.as_ref())
                    }
                    _ => (None, None, None),
                };

                let event = ConversationEvent {
                    r#type: "mcp_tool_call_update".to_string(),
                    data: serde_json::to_value(MCPToolCallUpdateEvent {
                        call_id,
                        conversation_id: self.conversation_id,
                        status: status_str,
                        result: result.map(|r| r.to_string()),
                        error: error.map(|e| e.to_string()),
                        started_time: None,
                        finished_time: finished_time,
                    }).unwrap(),
                };

                let _ = self.window.emit(
                    format!("conversation_event_{}", self.conversation_id).as_str(),
                    event
                );
            }

            // Agent execution plan - log only, no UI support yet
            acp::SessionUpdate::Plan(plan) => {
                info!("ACP Plan: {} entries", plan.entries.len());
                // TODO: Add frontend support for agent plan display
            }

            // Available commands update - log only, no UI support yet
            acp::SessionUpdate::AvailableCommandsUpdate(commands_update) => {
                info!("ACP AvailableCommandsUpdate: {} commands", commands_update.available_commands.len());
            }

            // Session mode change - log only, no UI support yet
            acp::SessionUpdate::CurrentModeUpdate(mode_update) => {
                info!("ACP CurrentModeUpdate: mode_id={:?}", mode_update.current_mode_id);
            }

            // Session info update - emit title change event (if feature enabled)
            #[cfg(feature = "unstable_session_info_update")]
            acp::SessionUpdate::SessionInfoUpdate(info_update) => {
                info!("ACP SessionInfoUpdate: title={:?}", info_update.title);

                // Check if title is defined (not undefined and not null)
                if !info_update.title.is_undefined() {
                    if let Some(title) = info_update.title.as_ref() {
                        let event = ConversationEvent {
                            r#type: "title_change".to_string(),
                            data: serde_json::json!({ "title": title }),
                        };

                        let _ = self.window.emit(
                            format!("conversation_event_{}", self.conversation_id).as_str(),
                            event
                        );
                    }
                }
            }

            // Config options update - log only (if feature enabled)
            #[cfg(feature = "unstable_session_config_options")]
            acp::SessionUpdate::ConfigOptionUpdate(config_update) => {
                debug!("ACP ConfigOptionUpdate: {:?}", config_update);
            }

            // Catch-all for any future variants
            _ => {
                debug!("ACP SessionNotification: unhandled variant");
            }
        }
        Ok(())
    }

    async fn request_permission(&self, args: acp::RequestPermissionRequest) -> acp::Result<acp::RequestPermissionResponse, acp::Error> {
        info!("ACP permission request: {:?}", args);

        // For now, we auto-deny permission requests for safety
        // TODO: Integrate with the permission manager to show UI dialogs
        Ok(acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled))
    }

    async fn write_text_file(&self, args: acp::WriteTextFileRequest) -> acp::Result<acp::WriteTextFileResponse, acp::Error> {
        info!("ACP write_text_file: path={}", args.path.display());

        let request = WriteFileRequest {
            file_path: args.path.to_string_lossy().to_string(),
            content: args.content,
        };

        match FileOperations::write_file(
            &self.operation_state,
            &self.permission_manager,
            request,
            self.get_conversation_id(),
        ).await {
            Ok(_) => {
                info!("File written successfully: {}", args.path.display());
                Ok(acp::WriteTextFileResponse::new())
            }
            Err(e) => {
                error!("Failed to write file: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn read_text_file(&self, args: acp::ReadTextFileRequest) -> acp::Result<acp::ReadTextFileResponse, acp::Error> {
        info!("ACP read_text_file: path={}", args.path.display());

        let request = ReadFileRequest {
            file_path: args.path.to_string_lossy().to_string(),
            offset: args.line.map(|l| l as usize),
            limit: args.limit.map(|l| l as usize),
        };

        match FileOperations::read_file(
            &self.operation_state,
            &self.permission_manager,
            request,
            self.get_conversation_id(),
        ).await {
            Ok(response) => {
                info!("File read successfully: {} bytes", response.content.len());
                Ok(acp::ReadTextFileResponse::new(response.content))
            }
            Err(e) => {
                error!("Failed to read file: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn create_terminal(&self, args: acp::CreateTerminalRequest) -> acp::Result<acp::CreateTerminalResponse, acp::Error> {
        info!("ACP create_terminal: command={}", args.command);

        // Build the full command with args
        let full_command = if args.args.is_empty() {
            args.command.clone()
        } else {
            format!("{} {}", args.command, args.args.join(" "))
        };

        // Create execute bash request
        let request = ExecuteBashRequest {
            command: full_command.clone(),
            description: Some(format!("ACP terminal: {}", full_command)),
            timeout: None,
            run_in_background: Some(true),
        };

        match BashOperations::execute_bash(&self.operation_state, request).await {
            Ok(response) => {
                let bash_id = response.bash_id.ok_or_else(|| {
                    acp::Error::internal_error().data("No bash_id returned for background process")
                })?;

                // Convert bash_id to TerminalId (wrap in Arc<str>)
                let terminal_id = acp::TerminalId::new(bash_id.clone());

                info!("Terminal created: terminal_id={}, bash_id={}", terminal_id.0, bash_id);
                Ok(acp::CreateTerminalResponse::new(terminal_id))
            }
            Err(e) => {
                error!("Failed to create terminal: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn terminal_output(&self, args: acp::TerminalOutputRequest) -> acp::Result<acp::TerminalOutputResponse, acp::Error> {
        debug!("ACP terminal_output: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        let request = GetBashOutputRequest {
            bash_id: bash_id.clone(),
            filter: None,
        };

        match BashOperations::get_bash_output(&self.operation_state, request).await {
            Ok(response) => {
                let exit_status = match response.status {
                    BashProcessStatus::Running => None,
                    BashProcessStatus::Completed | BashProcessStatus::Error => {
                        response.exit_code.map(|code| {
                            acp::TerminalExitStatus::new().exit_code(Some(code as u32))
                        })
                    }
                };

                Ok(acp::TerminalOutputResponse::new(response.output, false)
                    .exit_status(exit_status))
            }
            Err(e) => {
                error!("Failed to get terminal output: {}", e);
                Err(acp::Error::internal_error().data(e))
            }
        }
    }

    async fn release_terminal(&self, args: acp::ReleaseTerminalRequest) -> acp::Result<acp::ReleaseTerminalResponse, acp::Error> {
        info!("ACP release_terminal: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        // Remove the bash process from state (this will kill the process)
        self.operation_state.remove_bash_process(&bash_id).await;

        info!("Terminal released: {}", bash_id);
        Ok(acp::ReleaseTerminalResponse::new())
    }

    async fn wait_for_terminal_exit(&self, args: acp::WaitForTerminalExitRequest) -> acp::Result<acp::WaitForTerminalExitResponse, acp::Error> {
        info!("ACP wait_for_terminal_exit: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        // Wait for the process to complete by polling the state
        loop {
            if !self.operation_state.bash_process_exists(&bash_id).await {
                // Process no longer exists
                break;
            }

            // Check if completed
            let (output, completed, exit_code) = {
                let processes = self.operation_state.bash_processes.lock().await;
                if let Some(info) = processes.get(&bash_id) {
                    (
                        info.output_buffer.clone(),
                        info.completed,
                        info.exit_code,
                    )
                } else {
                    break;
                }
            };

            if completed {
                let exit_status = acp::TerminalExitStatus::new()
                    .exit_code(exit_code.map(|c| c as u32));
                info!("Terminal exited: terminal_id={}, exit_code={:?}", bash_id, exit_code);
                return Ok(acp::WaitForTerminalExitResponse::new(exit_status));
            }

            // Wait a bit before checking again
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        // If we get here, the process was removed without proper completion
        let exit_status = acp::TerminalExitStatus::new();
        Ok(acp::WaitForTerminalExitResponse::new(exit_status))
    }

    async fn kill_terminal_command(&self, args: acp::KillTerminalCommandRequest) -> acp::Result<acp::KillTerminalCommandResponse, acp::Error> {
        info!("ACP kill_terminal_command: terminal_id={}", args.terminal_id.0);

        let bash_id = args.terminal_id.0.to_string();

        // Remove the process which will kill it
        self.operation_state.remove_bash_process(&bash_id).await;

        info!("Terminal command killed: {}", bash_id);
        Ok(acp::KillTerminalCommandResponse::new())
    }

    async fn ext_method(&self, args: acp::ExtRequest) -> acp::Result<acp::ExtResponse, acp::Error> {
        info!("ACP ext_method: method={}, params={:?}", args.method, args.params);

        // For now, return NULL response
        // Custom extensions can be implemented here as needed
        Ok(acp::ExtResponse::new(serde_json::value::RawValue::NULL.to_owned().into()))
    }

    async fn ext_notification(&self, args: acp::ExtNotification) -> acp::Result<(), acp::Error> {
        debug!("ACP ext_notification: method={}, params={:?}", args.method, args.params);
        Ok(())
    }
}

/// Execute an ACP session
pub async fn execute_acp_session(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    conversation_id: i64,
    message_id: i64,
    user_prompt: &str,
    acp_config: AcpConfig,
) -> Result<(), AppError> {
    info!(
        "execute_acp_session: START - conversation_id={}, message_id={}, prompt='{}'",
        conversation_id, message_id, user_prompt
    );

    // Helper function to send error event
    let send_error = |window: &tauri::Window, msg: &str| {
        // Update database with error
        if let Ok(db) = ConversationDatabase::new(&app_handle) {
            if let Ok(repo) = db.message_repo() {
                let _ = repo.update_content(message_id, msg);
            }
        }

        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "error".to_string(),
                content: msg.to_string(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            }).unwrap(),
        };
        let event_name = format!("conversation_event_{}", conversation_id);
        let _ = window.emit(&event_name, event);
    };

    // Helper function to send done event
    let send_done = |window: &tauri::Window, content: &str| {
        let event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "response".to_string(),
                content: content.to_string(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            }).unwrap(),
        };
        let event_name = format!("conversation_event_{}", conversation_id);
        let _ = window.emit(&event_name, event);
    };

    // Create operation state and permission manager for this session
    let operation_state = Arc::new(OperationState::new());
    let permission_manager = Arc::new(PermissionManager::new(app_handle.clone()));

    // Resolve the actual executable path
    // For bun-installed packages, they are in ~/.bun/bin/
    let resolved_cli_command = resolve_acp_cli_path(&acp_config.cli_command);
    info!("ACP: Original CLI command: {}", acp_config.cli_command);
    info!("ACP: Resolved CLI path: {}", resolved_cli_command.display());

    // Build the command
    let full_command = if acp_config.additional_args.is_empty() {
        resolved_cli_command.display().to_string()
    } else {
        format!("{} {}", resolved_cli_command.display(), acp_config.additional_args.join(" "))
    };
    info!("ACP: Full command: {}", full_command);
    info!("ACP: Working directory: {}", acp_config.working_directory.display());

    let mut cmd = Command::new(&resolved_cli_command);
    cmd.current_dir(&acp_config.working_directory)
       .stdin(std::process::Stdio::piped())
       .stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::piped())
       .kill_on_drop(true);

    // Add environment variables
    for (key, value) in &acp_config.env_vars {
        cmd.env(key, value);
        debug!("ACP: Set env var: {}={}", key, if key.to_lowercase().contains("key") || key.to_lowercase().contains("token") { "***" } else { value });
    }
    if !acp_config.env_vars.is_empty() {
        info!("ACP: Environment variables set: {}", acp_config.env_vars.len());
    }

    // Add additional arguments
    if !acp_config.additional_args.is_empty() {
        cmd.args(&acp_config.additional_args);
        info!("ACP: Additional args: {:?}", acp_config.additional_args);
    }

    // Spawn the process
    info!("ACP: Spawning process...");
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            // 提供更友好的安装帮助信息
            let help_msg = match acp_config.cli_command.as_str() {
                "claude-code-acp" => "\n\n安装方法: bun add -g @zed-industries/claude-code-acp\n注意: 需要设置 ANTHROPIC_API_KEY 环境变量",
                "codex-acp" => "\n\n安装方法: bun add -g @zed-industries/codex-acp",
                "gemini" => "\n\n安装方法: 请参考 Google Gemini CLI 官方文档",
                _ => "",
            };
            let err_msg = format!(
                "无法启动 ACP 进程 '{}' (resolved: {}): {}{}", 
                acp_config.cli_command,
                resolved_cli_command.display(),
                e,
                help_msg
            );
            error!("ACP: {}", err_msg);
            send_error(&window, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };
    info!("ACP: Process spawned successfully, PID={:?}", child.id());

    let stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            let err_msg = "Failed to open stdin for ACP process".to_string();
            error!("ACP: {}", err_msg);
            send_error(&window, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };
    let stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            let err_msg = "Failed to open stdout for ACP process".to_string();
            error!("ACP: {}", err_msg);
            send_error(&window, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };
    let stderr = match child.stderr.take() {
        Some(s) => s,
        None => {
            let err_msg = "Failed to open stderr for ACP process".to_string();
            error!("ACP: {}", err_msg);
            send_error(&window, &err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
    };

    // Create shared buffers that will be accessible after the session
    let response_buffer = Arc::new(TokioMutex::new(String::new()));
    let reasoning_buffer = Arc::new(TokioMutex::new(String::new()));
    let response_buffer_clone = response_buffer.clone();
    let app_handle_for_db = app_handle.clone();

    // Create the ACP client with state
    let client_impl = AcpTauriClient {
        app_handle: app_handle.clone(),
        conversation_id,
        message_id,
        window: window.clone(),
        operation_state,
        permission_manager,
        response_content_buffer: response_buffer,
        reasoning_content_buffer: reasoning_buffer,
    };

    // Use LocalSet for !Send futures
    let local_set = tokio::task::LocalSet::new();
    let window_for_local = window.clone();

    let result = local_set.run_until(async move {
        info!("ACP: Creating ClientSideConnection...");
        let (conn, handle_io) = ClientSideConnection::new(
            client_impl,
            stdin.compat_write(),
            stdout.compat(),
            |fut| { tokio::task::spawn_local(fut); },
        );

        // Handle I/O in background with logging
        let io_handle = tokio::task::spawn_local(async move {
            info!("ACP I/O: Starting I/O handler...");
            handle_io.await;
            info!("ACP I/O: I/O handler finished");
        });
        info!("ACP: I/O handler spawned");

        // Spawn stderr reader to capture any error/debug output from the ACP process
        let _stderr_task = tokio::task::spawn_local(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};

            let mut stderr_reader = BufReader::new(stderr).lines();
            loop {
                match stderr_reader.next_line().await {
                    Ok(Some(line)) => {
                        info!("[ACP stderr] {}", line);
                    }
                    Ok(None) => {
                        info!("[ACP stderr] Stream closed (EOF)");
                        break;
                    }
                    Err(e) => {
                        error!("[ACP stderr] Read error: {}", e);
                        break;
                    }
                }
            }
        });
        info!("ACP: Stderr reader spawned");

        // Initialize with timeout
        info!("ACP: Initializing connection (timeout: 30s)...");
        let init_future = conn.initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                .client_info(acp::Implementation::new("AIPP", "0.4.1"))
        );
        
        let init_response = tokio::time::timeout(
            tokio::time::Duration::from_secs(30),
            init_future
        ).await;

        let init_response = match init_response {
            Ok(result) => result,
            Err(_) => {
                let err_msg = "ACP initialize timed out after 30 seconds. The CLI might not support ACP protocol or needs '--mcp' flag.".to_string();
                error!("ACP: {}", err_msg);
                return Err(AppError::UnknownError(err_msg));
            }
        };

        if let Err(e) = &init_response {
            let err_msg = format!("ACP initialize failed: {:?}", e);
            error!("ACP: {}", err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
        let _init_response = init_response.unwrap();
        info!("ACP: Initialize success, protocol_version={:?}", _init_response.protocol_version);

        // Create session
        info!("ACP: Creating session...");
        let session_response = conn.new_session(
            acp::NewSessionRequest::new(acp_config.working_directory.clone())
        ).await;

        if let Err(e) = &session_response {
            let err_msg = format!("ACP new_session failed: {:?}", e);
            error!("ACP: {}", err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
        let session_response = session_response.unwrap();
        info!("ACP: Session created, session_id={:?}", session_response.session_id);

        // Send prompt
        info!("ACP: Sending prompt...");
        let prompt_response = conn.prompt(
            acp::PromptRequest::new(session_response.session_id, vec![user_prompt.into()])
        ).await;

        if let Err(e) = &prompt_response {
            let err_msg = format!("ACP prompt failed: {:?}", e);
            error!("ACP: {}", err_msg);
            return Err(AppError::UnknownError(err_msg));
        }
        info!("ACP: Prompt completed successfully");

        // Prompt 完成后，获取最终内容并更新数据库
        // 注意：不等待进程退出，因为某些 ACP agent（如 Claude Code ACP）
        // 可能会保持会话活跃等待下一个 prompt
        let final_content = {
            let content = response_buffer_clone.lock().await.clone();
            info!("ACP: Final content length: {}", content.len());
            
            // 更新数据库（使用保存的 app_handle）
            if let Ok(db) = crate::ConversationDatabase::new(&app_handle_for_db) {
                if let Ok(repo) = db.message_repo() {
                    if let Err(e) = repo.update_content(message_id, &content) {
                        error!("ACP: Failed to update message content: {:?}", e);
                    } else {
                        info!("ACP: Final content saved to database");
                    }
                }
            }
            
            content
        };
        
        // 发送完成事件（在 local_set 内部发送，确保消息能及时到达）
        let done_event = ConversationEvent {
            r#type: "message_update".to_string(),
            data: serde_json::to_value(MessageUpdateEvent {
                message_id,
                message_type: "response".to_string(),
                content: final_content.clone(),
                is_done: true,
                token_count: None,
                input_token_count: None,
                output_token_count: None,
                ttft_ms: None,
                tps: None,
            }).unwrap(),
        };
        let event_name = format!("conversation_event_{}", conversation_id);
        if let Err(e) = window.emit(&event_name, done_event) {
            error!("ACP: Failed to emit done event: {:?}", e);
        } else {
            info!("ACP: Done event emitted");
        }

        // 终止进程（可选，但推荐）
        // 因为我们只发送了一个 prompt，之后不再需要这个会话
        info!("ACP: Killing child process...");
        if let Err(e) = child.kill().await {
            // 进程可能已经退出，忽略错误
            debug!("ACP: Kill process result: {:?}", e);
        }

        Ok::<(), AppError>(())
    }).await;

    // Handle result
    match result {
        Ok(()) => {
            info!("ACP: Session completed successfully");
        }
        Err(e) => {
            let err_msg = format!("{}", e);
            error!("ACP: Session failed: {}", err_msg);
            send_error(&window_for_local, &err_msg);
            return Err(e);
        }
    }

    info!("execute_acp_session: END - conversation_id={}", conversation_id);
    Ok(())
}

/// Extract ACP configuration from assistant_model_config and llm_provider_config
/// 
/// Configuration priority:
/// 1. assistant_model_config (assistant-level override)
/// 2. llm_provider_config (provider-level default)
/// 3. hardcoded default
pub fn extract_acp_config(
    model_configs: &[AssistantModelConfig],
    provider_configs: &[LLMProviderConfig],
) -> Result<AcpConfig, AppError> {
    use std::path::PathBuf;

    // Helper to get value from provider_configs
    let get_provider_config = |name: &str| -> Option<String> {
        provider_configs
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.value.clone())
    };

    // Helper to get value from model_configs
    let get_model_config = |name: &str| -> Option<String> {
        model_configs
            .iter()
            .find(|c| c.name == name)
            .and_then(|c| c.value.clone())
    };

    // 获取 CLI 命令
    // 只从 llm_provider_config 获取，因为这是提供商级别的配置
    // 注意：不同的 agent 需要不同的命令：
    // - Claude Code: 需要安装 @zed-industries/claude-code-acp，命令是 "claude-code-acp"
    // - Codex: 需要安装 @zed-industries/codex-acp，命令是 "codex-acp"
    // - Gemini: 原生支持 ACP，命令是 "gemini"
    let cli_command = get_provider_config("acp_cli_command")
        .unwrap_or_else(|| "claude-code-acp".to_string());
    
    debug!("ACP: cli_command from provider_config: {:?}", get_provider_config("acp_cli_command"));
    debug!("ACP: final cli_command: {}", cli_command);

    // 获取工作目录
    // 优先级: assistant_model_config > llm_provider_config > home_dir
    let working_directory = get_model_config("acp_working_directory")
        .or_else(|| get_provider_config("acp_working_directory"))
        .map(|p| PathBuf::from(p))
        .unwrap_or_else(|| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        });

    // 收集环境变量
    // 从两个配置源收集，model_config 优先级更高
    let mut env_vars = HashMap::new();
    
    // 先从 provider_configs 收集
    for config in provider_configs {
        if let Some(key) = config.name.strip_prefix("acp_env_") {
            env_vars.insert(key.to_uppercase(), config.value.clone());
        }
    }
    
    // 再从 model_configs 收集（会覆盖 provider 的同名配置）
    for config in model_configs {
        if let Some(key) = config.name.strip_prefix("acp_env_") {
            if let Some(value) = &config.value {
                env_vars.insert(key.to_uppercase(), value.clone());
            }
        }
    }

    // 获取额外参数
    // 优先级: assistant_model_config > llm_provider_config > empty
    let additional_args = get_model_config("acp_additional_args")
        .or_else(|| get_provider_config("acp_additional_args"))
        .map(|args| {
            args.split_whitespace()
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    // Log the extracted configuration for debugging
    info!(
        "extract_acp_config: cli_command='{}', working_directory='{}', env_vars={}, additional_args={:?}",
        cli_command,
        working_directory.display(),
        env_vars.len(),
        additional_args
    );

    Ok(AcpConfig {
        cli_command,
        working_directory,
        env_vars,
        additional_args,
    })
}
