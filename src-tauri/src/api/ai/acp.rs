//! ACP (Agent Client Protocol) integration module
//! Handles communication with ACP-compatible agents via stdio

use agent_client_protocol::{self as acp, Agent as _, Client as AcpClient, ClientSideConnection};
use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::db::assistant_db::AssistantModelConfig;
use crate::errors::AppError;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::Emitter;
use tokio::process::Command;
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::error;

/// ACP configuration extracted from assistant_model_config
#[derive(Debug, Clone)]
pub struct AcpConfig {
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub additional_args: Vec<String>,
}

/// Tauri client implementation that forwards ACP events to the frontend
pub struct AcpTauriClient {
    pub app_handle: tauri::AppHandle,
    pub conversation_id: i64,
    pub message_id: i64,
    pub window: tauri::Window,
}

impl AcpTauriClient {
    pub fn new(
        app_handle: tauri::AppHandle,
        conversation_id: i64,
        message_id: i64,
        window: tauri::Window,
    ) -> Self {
        Self {
            app_handle,
            conversation_id,
            message_id,
            window,
        }
    }
}

#[async_trait::async_trait(?Send)]
impl AcpClient for AcpTauriClient {
    async fn session_notification(&self, args: acp::SessionNotification) -> acp::Result<(), acp::Error> {
        match args.update {
            acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk { content, .. }) => {
                let text = match content {
                    acp::ContentBlock::Text(text_content) => text_content.text.clone(),
                    acp::ContentBlock::Image(_) => String::new(),
                    acp::ContentBlock::Audio(_) => String::new(),
                    acp::ContentBlock::ResourceLink(resource_link) => resource_link.uri.clone(),
                    acp::ContentBlock::Resource(_) => String::new(),
                    _ => String::new(),
                };

                let event = ConversationEvent {
                    r#type: "message_update".to_string(),
                    data: serde_json::to_value(MessageUpdateEvent {
                        message_id: self.message_id,
                        message_type: "response".to_string(),
                        content: text,
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
            _ => {}
        }
        Ok(())
    }

    async fn request_permission(&self, _args: acp::RequestPermissionRequest) -> acp::Result<acp::RequestPermissionResponse, acp::Error> {
        // For now, cancel permission requests (auto-deny for safety)
        Ok(acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled))
    }

    async fn write_text_file(&self, _args: acp::WriteTextFileRequest) -> acp::Result<acp::WriteTextFileResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn read_text_file(&self, _args: acp::ReadTextFileRequest) -> acp::Result<acp::ReadTextFileResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn create_terminal(&self, _args: acp::CreateTerminalRequest) -> acp::Result<acp::CreateTerminalResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn terminal_output(&self, _args: acp::TerminalOutputRequest) -> acp::Result<acp::TerminalOutputResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn release_terminal(&self, _args: acp::ReleaseTerminalRequest) -> acp::Result<acp::ReleaseTerminalResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn wait_for_terminal_exit(&self, _args: acp::WaitForTerminalExitRequest) -> acp::Result<acp::WaitForTerminalExitResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn kill_terminal_command(&self, _args: acp::KillTerminalCommandRequest) -> acp::Result<acp::KillTerminalCommandResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_method(&self, _args: acp::ExtRequest) -> acp::Result<acp::ExtResponse, acp::Error> {
        Err(acp::Error::method_not_found())
    }

    async fn ext_notification(&self, _args: acp::ExtNotification) -> acp::Result<(), acp::Error> {
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
    // Build the command
    let mut cmd = Command::new(&acp_config.cli_command);
    cmd.current_dir(&acp_config.working_directory)
       .stdin(std::process::Stdio::piped())
       .stdout(std::process::Stdio::piped())
       .stderr(std::process::Stdio::piped())
       .kill_on_drop(true);

    // Add environment variables
    for (key, value) in &acp_config.env_vars {
        cmd.env(key, value);
    }

    // Add additional arguments
    if !acp_config.additional_args.is_empty() {
        cmd.args(&acp_config.additional_args);
    }

    // Spawn the process
    let mut child = cmd.spawn()
        .map_err(|e| AppError::UnknownError(format!("Failed to spawn ACP process: {}", e)))?;

    let stdin = child.stdin.take()
        .ok_or_else(|| AppError::UnknownError("Failed to open stdin".to_string()))?;
    let stdout = child.stdout.take()
        .ok_or_else(|| AppError::UnknownError("Failed to open stdout".to_string()))?;

    // Create the ACP client
    let client_impl = AcpTauriClient::new(
        app_handle.clone(),
        conversation_id,
        message_id,
        window.clone(),
    );

    // Use LocalSet for !Send futures
    let local_set = tokio::task::LocalSet::new();

    local_set.run_until(async move {
        let (conn, handle_io) = ClientSideConnection::new(
            client_impl,
            stdin.compat_write(),
            stdout.compat(),
            |fut| { tokio::task::spawn_local(fut); },
        );

        // Handle I/O in background
        tokio::task::spawn_local(handle_io);

        // Initialize
        let _init_response = conn.initialize(
            acp::InitializeRequest::new(acp::ProtocolVersion::V1)
                .client_info(acp::Implementation::new("AIPP", "0.4.1"))
        ).await
        .map_err(|e| AppError::UnknownError(format!("ACP initialize failed: {}", e)))?;

        // Create session
        let session_response = conn.new_session(
            acp::NewSessionRequest::new(acp_config.working_directory)
        ).await
        .map_err(|e| AppError::UnknownError(format!("ACP new_session failed: {}", e)))?;

        // Send prompt
        let prompt_response = conn.prompt(
            acp::PromptRequest::new(session_response.session_id, vec![user_prompt.into()])
        ).await;

        if let Err(e) = prompt_response {
            error!("ACP prompt failed: {}", e);
        }

        // Wait for process to finish
        let _ = child.wait().await;

        Ok::<(), AppError>(())
    }).await?;

    Ok(())
}

/// Extract ACP configuration from assistant_model_config
pub fn extract_acp_config(
    model_configs: &[AssistantModelConfig],
) -> Result<AcpConfig, AppError> {
    use std::path::PathBuf;

    let cli_command = model_configs
        .iter()
        .find(|c| c.name == "acp_cli_command")
        .and_then(|c| c.value.clone())
        .unwrap_or_else(|| "claude".to_string());

    let working_directory = model_configs
        .iter()
        .find(|c| c.name == "acp_working_directory")
        .and_then(|c| c.value.clone())
        .map(|p| PathBuf::from(p))
        .unwrap_or_else(|| {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        });

    let mut env_vars = HashMap::new();
    for config in model_configs {
        if let Some(key) = config.name.strip_prefix("acp_env_") {
            if let Some(value) = &config.value {
                env_vars.insert(key.to_uppercase(), value.clone());
            }
        }
    }

    let additional_args = model_configs
        .iter()
        .find(|c| c.name == "acp_additional_args")
        .and_then(|c| c.value.clone())
        .map(|args| {
            args.split_whitespace()
                .map(|s| s.to_string())
                .collect()
        })
        .unwrap_or_default();

    Ok(AcpConfig {
        cli_command,
        working_directory,
        env_vars,
        additional_args,
    })
}
