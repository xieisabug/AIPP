use crate::api::ai::events::{ConversationEvent, MessageUpdateEvent};
use crate::api::ai::acp::{
    resolve_acp_cli_path, AcpPermissionDecision, AcpPermissionOptionPayload, AcpPermissionRequestEvent,
    AcpPermissionState,
};
use crate::api::operation_api::{
    emit_permission_request_event, ACP_PERMISSION_REQUEST_EVENT,
};
use crate::acp_mcp_bridge::{
    ensure_proxy_server, AcpMcpProxyConfig, ACP_MCP_BRIDGE_ARG, ACP_MCP_CONVERSATION_ID_ENV,
    ACP_MCP_DB_PATH_ENV, ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV, ACP_MCP_PROXY_ADDR_ENV,
    ACP_MCP_PROXY_TOKEN_ENV, ACP_MCP_SELECTED_TOOLS_ENV,
};
use crate::db::conversation_db::{ConversationDatabase, Repository};
use crate::db::mcp_db::MCPDatabase;
use crate::errors::AppError;
use crate::mcp::builtin_mcp::interaction::{
    request_ask_user_question, AskUserQuestionItem, AskUserQuestionMetadata,
    AskUserQuestionOption, AskUserQuestionRequest, InteractionState,
};
use crate::state::activity_state::ConversationActivityManager;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::process::Stdio;
use tauri::{Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdout, Command};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, error, info, warn};

pub const CODEX_APP_SERVER_API_TYPE: &str = "codex_app_server";

#[derive(Debug, Clone)]
pub struct CodexAppServerConfig {
    pub cli_command: String,
    pub working_directory: PathBuf,
    pub env_vars: HashMap<String, String>,
    pub additional_args: Vec<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub approval_policy: Option<String>,
    pub sandbox: Option<String>,
    pub selected_mcp_tools_payload: String,
    pub session_signature: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodexConversationSessionState {
    pub conversation_id: i64,
    pub agent_kind: String,
    pub session_id: Option<String>,
    pub load_session_supported: bool,
    pub session_resume_supported: bool,
    pub restored_session_method: Option<String>,
    pub current_turn_id: Option<String>,
    pub has_active_prompt: bool,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub config_options: Vec<CodexSessionConfigOption>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionConfigOption {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub category: Option<String>,
    pub current_value: String,
    pub options: Vec<CodexSessionConfigChoice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexSessionConfigChoice {
    pub value: String,
    pub name: String,
    pub description: Option<String>,
    pub group_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CodexSessionStateSnapshotEvent {
    state: Option<CodexConversationSessionState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentActivityEvent {
    pub conversation_id: i64,
    pub response_message_id: i64,
    pub agent_kind: String,
    pub session_id: Option<String>,
    pub item_id: String,
    pub sequence: u64,
    pub kind: String,
    pub status: String,
    pub title: Option<String>,
    pub input: Option<Value>,
    pub output: Option<String>,
    pub error: Option<String>,
    pub metadata: Value,
    /// item 开始时已输出的正文字符数（按 Unicode 字符计），用于前端把活动卡片穿插到正文对应位置；
    /// 旧数据没有该字段时前端回退为「全部列在正文之后」
    pub content_offset: Option<u64>,
}

enum CodexSessionCommand {
    Prompt { message_id: i64, prompt: String, window: tauri::Window },
    CancelCurrentPrompt { response: oneshot::Sender<Result<(), String>> },
    SetConfigOption { config_id: String, value: String, response: oneshot::Sender<Result<(), String>> },
}

#[derive(Clone)]
pub struct CodexSessionHandle {
    sender: mpsc::UnboundedSender<CodexSessionCommand>,
}

impl CodexSessionHandle {
    pub fn send_prompt(
        &self,
        message_id: i64,
        prompt: String,
        window: tauri::Window,
    ) -> Result<(), AppError> {
        self.sender
            .send(CodexSessionCommand::Prompt { message_id, prompt, window })
            .map_err(|_| AppError::UnknownError("Codex app-server session closed".to_string()))
    }

    pub async fn cancel_current_prompt(&self) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender
            .send(CodexSessionCommand::CancelCurrentPrompt { response: tx })
            .map_err(|_| AppError::UnknownError("Codex app-server session closed".to_string()))?;
        rx.await
            .map_err(|_| AppError::UnknownError("Codex app-server session closed".to_string()))?
            .map_err(AppError::UnknownError)
    }

    pub async fn set_config_option(&self, config_id: String, value: String) -> Result<(), AppError> {
        let (tx, rx) = oneshot::channel();
        self.sender.send(CodexSessionCommand::SetConfigOption { config_id, value, response: tx })
            .map_err(|_| AppError::UnknownError("Codex app-server session closed".to_string()))?;
        rx.await.map_err(|_| AppError::UnknownError("Codex app-server session closed".to_string()))?
            .map_err(AppError::UnknownError)
    }
}

pub struct CodexSessionEntry {
    pub handle: CodexSessionHandle,
    pub snapshot: CodexConversationSessionState,
    pub config_signature: String,
}

impl CodexSessionEntry {
    pub fn new(handle: CodexSessionHandle, conversation_id: i64, config_signature: String) -> Self {
        Self {
            handle,
            snapshot: CodexConversationSessionState {
                conversation_id,
                agent_kind: CODEX_APP_SERVER_API_TYPE.to_string(),
                ..Default::default()
            },
            config_signature,
        }
    }
}

/// 解析环境变量配置：优先按 JSON 对象解析（provider 表单使用 JSON 格式），
/// 否则按每行 KEY=VALUE 解析（与 ACP 通道及助手级配置的格式一致）
fn merge_codex_env_blob(env_vars: &mut HashMap<String, String>, raw: &str) {
    if let Ok(parsed) = serde_json::from_str::<HashMap<String, String>>(raw) {
        env_vars.extend(parsed);
        return;
    }
    crate::api::ai::acp::merge_acp_env_blob(env_vars, raw);
}

pub fn extract_codex_app_server_config(
    model_configs: &[crate::db::assistant_db::AssistantModelConfig],
    provider_configs: &[crate::db::llm_db::LLMProviderConfig],
    model: Option<String>,
) -> Result<CodexAppServerConfig, AppError> {
    let provider_value = |name: &str| {
        provider_configs
            .iter()
            .find(|config| config.name == name)
            .map(|config| config.value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let model_value = |name: &str| {
        model_configs
            .iter()
            .find(|config| config.name == name)
            .and_then(|config| config.value.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
    };

    let cli_command = provider_value("codex_cli_command").unwrap_or_else(|| "codex".to_string());
    let working_directory = model_value("acp_working_directory")
        .or_else(|| provider_value("acp_working_directory"))
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")));
    // 与 ACP 通道一致：assistant_model_config（助手级）优先于 provider 默认值
    let additional_args = model_value("acp_additional_args")
        .or_else(|| provider_value("codex_additional_args"))
        .map(|raw| raw.split_whitespace().map(str::to_string).collect())
        .unwrap_or_default();
    let mut env_vars = HashMap::new();
    if let Some(home) = dirs::home_dir() {
        let codex_home = home.join(".codex");
        if codex_home.exists() {
            env_vars.insert("CODEX_HOME".to_string(), codex_home.display().to_string());
        }
    }
    for raw in [provider_value("acp_env_vars"), model_value("acp_env_vars")].into_iter().flatten() {
        merge_codex_env_blob(&mut env_vars, &raw);
    }
    let approval_policy = model_value("codex_approval_policy")
        .or_else(|| provider_value("codex_approval_policy"));
    let sandbox = model_value("codex_sandbox").or_else(|| provider_value("codex_sandbox"));
    let mut config = CodexAppServerConfig {
        cli_command,
        working_directory,
        env_vars,
        additional_args,
        model,
        reasoning_effort: model_value("reasoning_effort"),
        approval_policy,
        sandbox,
        selected_mcp_tools_payload: String::new(),
        session_signature: String::new(),
    };
    config.session_signature = codex_config_signature(&config);
    Ok(config)
}

fn codex_config_signature(config: &CodexAppServerConfig) -> String {
    serde_json::to_string(&json!({
        "cli": config.cli_command,
        "cwd": config.working_directory,
        "args": config.additional_args,
        "model": config.model,
        "reasoning_effort": config.reasoning_effort,
        "approval": config.approval_policy,
        "sandbox": config.sandbox,
        "env": config.env_vars,
        "selected_mcp": config.selected_mcp_tools_payload,
    }))
    .unwrap_or_default()
}

pub fn refresh_codex_session_signature(config: &mut CodexAppServerConfig) {
    config.session_signature = codex_config_signature(config);
}

fn emit_session_snapshot(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    state: Option<CodexConversationSessionState>,
) {
    let event = ConversationEvent {
        r#type: "acp_session_state_snapshot".to_string(),
        data: serde_json::to_value(CodexSessionStateSnapshotEvent { state }).unwrap(),
    };
    let _ = crate::utils::window_utils::send_conversation_event_to_chat_windows(
        app_handle,
        conversation_id,
        event,
    );
}

fn json_rpc_request(id: u64, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

/// 构造 Codex thread 的 MCP config 覆盖：把 AIPP 桥接进程注册为名为 `aipp` 的 MCP server。
/// 通过 thread/start 与 thread/resume 的 `config` 字段下发（Codex 0.149 已验证），
/// 不修改用户 ~/.codex/config.toml；与 ACP 通道共用同一桥接与代理。
fn build_codex_mcp_config_overrides(
    bridge_command: &std::path::Path,
    mcp_db_path: &std::path::Path,
    conversation_id: i64,
    proxy: &AcpMcpProxyConfig,
    selected_mcp_tools_payload: &str,
) -> serde_json::Map<String, Value> {
    let mut env = serde_json::Map::new();
    env.insert(ACP_MCP_DB_PATH_ENV.to_string(), json!(mcp_db_path.display().to_string()));
    env.insert(ACP_MCP_CONVERSATION_ID_ENV.to_string(), json!(conversation_id.to_string()));
    env.insert(ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV.to_string(), json!("1"));
    env.insert(ACP_MCP_PROXY_ADDR_ENV.to_string(), json!(proxy.addr));
    env.insert(ACP_MCP_PROXY_TOKEN_ENV.to_string(), json!(proxy.token));
    env.insert(ACP_MCP_SELECTED_TOOLS_ENV.to_string(), json!(selected_mcp_tools_payload));
    let mut overrides = serde_json::Map::new();
    overrides.insert("mcp_servers.aipp.command".to_string(), json!(bridge_command.display().to_string()));
    overrides.insert("mcp_servers.aipp.args".to_string(), json!([ACP_MCP_BRIDGE_ARG]));
    overrides.insert("mcp_servers.aipp.env".to_string(), Value::Object(env));
    overrides.insert("mcp_servers.aipp.startup_timeout_sec".to_string(), json!(20));
    overrides
}

/// 助手选中了 MCP 工具时生成 thread 级 config 覆盖；未选中时不挂载桥接
async fn codex_mcp_bridge_overrides(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    config: &CodexAppServerConfig,
) -> Result<Option<serde_json::Map<String, Value>>, String> {
    let payload = config.selected_mcp_tools_payload.trim();
    if payload.is_empty() || payload == "[]" {
        return Ok(None);
    }
    let bridge_command = std::env::current_exe()
        .map_err(|error| format!("Failed to resolve AIPP executable for Codex MCP bridge: {error}"))?;
    let mcp_db_path = MCPDatabase::db_path(app_handle)?;
    let proxy = ensure_proxy_server(app_handle.clone()).await?;
    Ok(Some(build_codex_mcp_config_overrides(
        &bridge_command,
        &mcp_db_path,
        conversation_id,
        &proxy,
        &config.selected_mcp_tools_payload,
    )))
}

fn rpc_id_key(id: &Value) -> String {
    id.as_str()
        .map(str::to_string)
        .unwrap_or_else(|| id.to_string())
}

fn codex_permission_options(method: &str, params: &Value) -> Vec<AcpPermissionOptionPayload> {
    let available = params.get("availableDecisions").and_then(Value::as_array);
    let mut decisions = available
        .map(|items| items.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .unwrap_or_default();
    if decisions.is_empty() {
        decisions = if method == "item/permissions/requestApproval" {
            vec!["accept", "acceptForSession", "decline"]
        } else {
            vec!["accept", "acceptForSession", "decline", "cancel"]
        };
    }
    decisions
        .into_iter()
        .map(|decision| AcpPermissionOptionPayload {
            option_id: decision.to_string(),
            name: match decision {
                "accept" => "本次允许",
                "acceptForSession" => "本会话允许",
                "decline" => "拒绝并继续",
                "cancel" => "拒绝并中止本轮",
                other => other,
            }
            .to_string(),
            kind: match decision {
                "accept" => "allow_once",
                "acceptForSession" => "allow_always",
                "cancel" => "reject_always",
                _ => "reject_once",
            }
            .to_string(),
        })
        .collect()
}

fn codex_approval_response(method: &str, params: &Value, decision: AcpPermissionDecision) -> Value {
    let selected = match decision {
        AcpPermissionDecision::Selected(value) => value,
        AcpPermissionDecision::Cancelled => "cancel".to_string(),
    };
    if method == "item/permissions/requestApproval" {
        if selected == "accept" || selected == "acceptForSession" {
            json!({
                "permissions": params.get("permissions").cloned().unwrap_or_else(|| json!({})),
                "scope": if selected == "acceptForSession" { "session" } else { "turn" }
            })
        } else {
            json!({"permissions": {}})
        }
    } else {
        json!({"decision": selected})
    }
}

async fn handle_approval_request(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    frame: &Value,
) -> Result<bool, String> {
    let Some(method) = frame.get("method").and_then(Value::as_str) else {
        return Ok(false);
    };
    if !matches!(
        method,
        "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    ) {
        return Ok(false);
    }
    let rpc_id = frame.get("id").cloned().ok_or("Codex approval request missing id")?;
    let params = frame.get("params").cloned().unwrap_or(Value::Null);
    let item_id = params
        .get("itemId")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let request_id = format!("codex:{conversation_id}:{}", rpc_id_key(&rpc_id));
    let title = params
        .get("command")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| params.get("reason").and_then(Value::as_str).map(str::to_string))
        .or_else(|| {
            Some(match method {
                "item/fileChange/requestApproval" => "允许 Codex 修改文件",
                "item/permissions/requestApproval" => "允许 Codex 扩展权限",
                _ => "允许 Codex 执行命令",
            }
            .to_string())
        });
    let event = AcpPermissionRequestEvent {
        request_id: request_id.clone(),
        conversation_id: Some(conversation_id),
        agent_kind: Some(CODEX_APP_SERVER_API_TYPE.to_string()),
        tool_call_id: item_id,
        title,
        kind: Some(method.to_string()),
        parameters: Some(serde_json::to_string_pretty(&params).unwrap_or_else(|_| params.to_string())),
        options: codex_permission_options(method, &params),
    };
    let (decision_tx, decision_rx) = oneshot::channel();
    let permission_state = app_handle.state::<AcpPermissionState>();
    permission_state.store_request(event.clone(), decision_tx).await;
    if let Err(error) = emit_permission_request_event(
        app_handle,
        ACP_PERMISSION_REQUEST_EVENT,
        Some(conversation_id),
        &event,
    ) {
        permission_state.remove_request(&request_id).await;
        return Err(error);
    }
    let decision = decision_rx
        .await
        .unwrap_or(AcpPermissionDecision::Cancelled);
    let result = codex_approval_response(method, &params, decision);
    write_frame(stdin, &json!({"jsonrpc":"2.0","id":rpc_id,"result":result})).await?;
    Ok(true)
}

fn supported_reasoning_efforts(value: &Value) -> Vec<String> {
    let mut result = Vec::new();
    fn visit(value: &Value, result: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key.eq_ignore_ascii_case("supportedReasoningEfforts") {
                        if let Some(items) = child.as_array() {
                            for item in items {
                                if let Some(name) = item.as_str() {
                                    if !result.iter().any(|existing| existing == name) {
                                        result.push(name.to_string());
                                    }
                                } else if let Some(name) = item.get("effort").or_else(|| item.get("reasoningEffort")).and_then(Value::as_str) {
                                    if !result.iter().any(|existing| existing == name) {
                                        result.push(name.to_string());
                                    }
                                }
                            }
                        }
                    }
                    visit(child, result);
                }
            }
            Value::Array(items) => items.iter().for_each(|item| visit(item, result)),
            _ => {}
        }
    }
    visit(value, &mut result);
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexModelCatalogEntry {
    pub id: String,
    pub name: String,
    pub supported_efforts: Vec<String>,
    pub default_effort: Option<String>,
}

fn model_catalog(value: &Value) -> Vec<CodexModelCatalogEntry> {
    let mut models = Vec::new();
    fn visit(value: &Value, models: &mut Vec<CodexModelCatalogEntry>) {
        match value {
            Value::Object(map) => {
                let id = map.get("id").or_else(|| map.get("model")).and_then(Value::as_str);
                let label = map.get("displayName").or_else(|| map.get("name")).and_then(Value::as_str);
                if let Some(id) = id {
                    if map.contains_key("supportedReasoningEfforts") || label.is_some() {
                        let entry = CodexModelCatalogEntry {
                            id: id.to_string(),
                            name: label.unwrap_or(id).to_string(),
                            supported_efforts: supported_reasoning_efforts(value),
                            default_effort: map.get("defaultReasoningEffort").and_then(Value::as_str).map(str::to_string),
                        };
                        if !models.iter().any(|existing| existing.id == entry.id) {
                            models.push(entry);
                        }
                    }
                }
                map.values().for_each(|child| visit(child, models));
            }
            Value::Array(items) => items.iter().for_each(|item| visit(item, models)),
            _ => {}
        }
    }
    visit(value, &mut models);
    models
}

/// 启动一次短生命周期 Codex app-server，仅探测真实的 model/list 目录。
/// 不创建 thread，也不会写入会话数据库。
pub async fn probe_codex_model_options(
    app_handle: &tauri::AppHandle,
    config: &CodexAppServerConfig,
) -> Result<Vec<CodexModelCatalogEntry>, String> {
    let (_child, mut stdin, mut lines) = spawn_process(config).await?;
    let mut pending_notifications = VecDeque::new();
    write_frame(
        &mut stdin,
        &json_rpc_request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"aipp-model-probe","title":"AIPP","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"experimentalApi":true}
            }),
        ),
    )
    .await?;
    read_rpc_response_with_approvals(
        app_handle,
        -1,
        &mut stdin,
        &mut lines,
        1,
        &mut pending_notifications,
    )
    .await?;
    write_frame(&mut stdin, &json!({"jsonrpc":"2.0","method":"initialized"})).await?;
    write_frame(&mut stdin, &json_rpc_request(2, "model/list", json!({}))).await?;
    let result = read_rpc_response_with_approvals(
        app_handle,
        -1,
        &mut stdin,
        &mut lines,
        2,
        &mut pending_notifications,
    )
    .await?;
    let catalog = model_catalog(&result);
    if catalog.is_empty() {
        return Err(format!("Codex model/list 返回空模型列表：{result}"));
    }
    Ok(catalog)
}

fn first_string_for_keys(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(Value::as_str) {
                    return Some(value.to_string());
                }
            }
            map.values().find_map(|child| first_string_for_keys(child, keys))
        }
        Value::Array(items) => items.iter().find_map(|item| first_string_for_keys(item, keys)),
        _ => None,
    }
}

fn codex_reasoning_options(efforts: &[String]) -> Vec<CodexSessionConfigChoice> {
    efforts.iter().map(|value| CodexSessionConfigChoice {
        value: value.clone(),
        name: match value.as_str() {
            "none" => "关闭思考".to_string(),
            other => other.to_ascii_uppercase(),
        },
        description: None,
        group_name: None,
    }).collect()
}

fn codex_model_choices(
    catalog: &[CodexModelCatalogEntry],
    current_model: Option<&str>,
) -> Vec<CodexSessionConfigChoice> {
    let mut choices: Vec<_> = catalog.iter().map(|entry| CodexSessionConfigChoice {
        value: entry.id.clone(),
        name: entry.name.clone(),
        description: None,
        group_name: None,
    }).collect();
    if let Some(current_model) = current_model {
        if !choices.iter().any(|choice| choice.value == current_model) {
            choices.insert(0, CodexSessionConfigChoice {
                value: current_model.to_string(),
                name: format!("{}（当前线程）", current_model),
                description: Some("该模型来自已恢复的 Codex 线程，当前 model/list 未返回它".to_string()),
                group_name: None,
            });
        }
    }
    choices
}

fn apply_codex_config_option(
    snapshot: &mut CodexConversationSessionState,
    catalog: &[CodexModelCatalogEntry],
    config_id: &str,
    value: String,
) -> Result<(), String> {
    if config_id == "model" {
        let model = catalog.iter().find(|model| model.id == value)
            .ok_or_else(|| format!("Codex 模型不在 model/list 返回结果中：{value}"))?;
        snapshot.model = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "model") {
            option.current_value = value;
        }
        let current_effort_supported = snapshot.reasoning_effort.as_ref()
            .is_some_and(|effort| model.supported_efforts.iter().any(|candidate| candidate == effort));
        if !current_effort_supported {
            snapshot.reasoning_effort = model.default_effort.clone().or_else(|| model.supported_efforts.first().cloned());
        }
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "reasoning_effort") {
            option.options = codex_reasoning_options(&model.supported_efforts);
            option.current_value = snapshot.reasoning_effort.clone().unwrap_or_default();
        }
        return Ok(());
    }
    if config_id == "reasoning_effort" || config_id == "thought_level" {
        let model = snapshot.model.as_ref().and_then(|id| catalog.iter().find(|model| &model.id == id));
        if !model.is_some_and(|model| model.supported_efforts.iter().any(|candidate| candidate == &value)) {
            return Err(format!("当前 Codex 模型不支持思考强度：{value}"));
        }
        snapshot.reasoning_effort = Some(value.clone());
        if let Some(option) = snapshot.config_options.iter_mut().find(|option| option.id == "reasoning_effort") {
            option.current_value = value;
        }
        return Ok(());
    }
    Err(format!("Codex 不支持会话配置：{config_id}"))
}

fn codex_turn_start_params(thread_id: &str, snapshot: &CodexConversationSessionState, prompt: &str) -> Value {
    json!({
        "threadId": thread_id,
        "model": snapshot.model,
        "reasoningEffort": snapshot.reasoning_effort,
        "input": [{"type":"text","text":prompt,"text_elements":[]}],
    })
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct CodexTurnUsage {
    input_tokens: i32,
    output_tokens: i32,
    reasoning_tokens: i32,
    cached_input_tokens: i32,
    cache_write_input_tokens: i32,
    total_tokens: i32,
}

fn codex_user_input_request(
    params: &Value,
) -> Result<(AskUserQuestionRequest, Vec<(String, String, bool)>), String> {
    let questions = params
        .get("questions")
        .and_then(Value::as_array)
        .ok_or("Codex requestUserInput is missing questions")?;
    let mut mappings = Vec::with_capacity(questions.len());
    let mut converted = Vec::with_capacity(questions.len());
    for question in questions {
        let id = question
            .get("id")
            .and_then(Value::as_str)
            .ok_or("Codex requestUserInput question is missing id")?
            .to_string();
        let text = question
            .get("question")
            .and_then(Value::as_str)
            .ok_or("Codex requestUserInput question is missing question")?
            .to_string();
        let header = question
            .get("header")
            .and_then(Value::as_str)
            .unwrap_or("Codex")
            .to_string();
        let options = question
            .get("options")
            .and_then(Value::as_array)
            .map(|options| {
                options
                    .iter()
                    .filter_map(|option| {
                        Some(AskUserQuestionOption {
                            label: option.get("label")?.as_str()?.to_string(),
                            description: option
                                .get("description")
                                .and_then(Value::as_str)
                                .unwrap_or("选择此项")
                                .to_string(),
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let multi_select = question
            .get("multiSelect")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        mappings.push((id, text.clone(), multi_select));
        converted.push(AskUserQuestionItem {
            question: text,
            header,
            options,
            multi_select,
            is_secret: question
                .get("isSecret")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        });
    }
    Ok((
        AskUserQuestionRequest {
            questions: converted,
            answers: None,
            metadata: Some(AskUserQuestionMetadata {
                source: Some("codex_app_server".to_string()),
            }),
        },
        mappings,
    ))
}

fn codex_user_input_response(
    mappings: &[(String, String, bool)],
    answers: &HashMap<String, String>,
) -> Value {
    let answers = mappings
        .iter()
        .filter_map(|(id, question, multi_select)| {
            let answer = answers.get(question)?.trim();
            if answer.is_empty() {
                return None;
            }
            let values = if *multi_select {
                answer
                    .split(',')
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            } else {
                vec![answer.to_string()]
            };
            Some((id.clone(), json!({"answers": values})))
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"answers": answers})
}

async fn handle_server_request(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    frame: &Value,
) -> Result<bool, String> {
    if handle_approval_request(app_handle, conversation_id, stdin, frame).await? {
        return Ok(true);
    }
    if frame.get("method").and_then(Value::as_str) != Some("item/tool/requestUserInput") {
        return Ok(false);
    }
    let rpc_id = frame.get("id").cloned().ok_or("Codex requestUserInput missing id")?;
    let params = frame.get("params").cloned().unwrap_or(Value::Null);
    let (request, mappings) = codex_user_input_request(&params)?;
    let interaction_state = app_handle.state::<InteractionState>();
    let result = match request_ask_user_question(
        app_handle,
        &interaction_state,
        Some(conversation_id),
        request,
    )
    .await
    {
        Ok(answers) => codex_user_input_response(&mappings, &answers),
        Err(error) => {
            warn!(conversation_id, error = %error, "Codex requestUserInput was cancelled");
            json!({"answers": {}})
        }
    };
    write_frame(stdin, &json!({"jsonrpc":"2.0","id":rpc_id,"result":result})).await?;
    Ok(true)
}

async fn write_frame(stdin: &mut tokio::process::ChildStdin, frame: &Value) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(frame).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await.map_err(|error| error.to_string())?;
    stdin.flush().await.map_err(|error| error.to_string())
}

async fn read_rpc_response_with_approvals(
    app_handle: &tauri::AppHandle,
    conversation_id: i64,
    stdin: &mut tokio::process::ChildStdin,
    lines: &mut Lines<BufReader<ChildStdout>>,
    expected_id: u64,
    pending_notifications: &mut VecDeque<Value>,
) -> Result<Value, String> {
    while let Some(line) = lines.next_line().await.map_err(|error| error.to_string())? {
        let frame: Value = serde_json::from_str(&line)
            .map_err(|error| format!("Invalid Codex app-server JSON: {error}: {line}"))?;
        if frame.get("method").is_some() && frame.get("id").is_some() {
            if !handle_server_request(app_handle, conversation_id, stdin, &frame).await? {
                warn!(
                    conversation_id,
                    method = frame.get("method").and_then(|value| value.as_str()).unwrap_or("unknown"),
                    "Unsupported Codex server request"
                );
                write_frame(
                    stdin,
                    &json!({
                        "jsonrpc":"2.0",
                        "id":frame.get("id").cloned().unwrap_or(Value::Null),
                        "error":{"code":-32601,"message":format!("AIPP does not support Codex server request {}", frame.get("method").and_then(Value::as_str).unwrap_or("unknown"))}
                    }),
                )
                .await?;
            }
            continue;
        }
        if frame.get("id").and_then(Value::as_u64) == Some(expected_id) {
            if let Some(error) = frame.get("error") {
                return Err(format!("Codex app-server RPC error: {error}"));
            }
            return Ok(frame.get("result").cloned().unwrap_or(Value::Null));
        }
        if frame.get("method").is_some() {
            pending_notifications.push_back(frame);
        }
    }
    Err("Codex app-server closed before replying".to_string())
}

fn thread_id_from_result(result: &Value) -> Option<String> {
    result
        .pointer("/thread/id")
        .or_else(|| result.get("threadId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn item_identity(item: &Value) -> (String, String, Option<String>, Option<Value>) {
    let item_id = item.get("id").and_then(Value::as_str).unwrap_or("unknown").to_string();
    let kind = item.get("type").and_then(Value::as_str).unwrap_or("status").to_string();
    match kind.as_str() {
        "commandExecution" => (
            item_id,
            "command".to_string(),
            item.get("command").and_then(Value::as_str).map(str::to_string),
            Some(json!({"command": item.get("command"), "cwd": item.get("cwd")})),
        ),
        "fileChange" => (item_id, "patch".to_string(), Some("文件变更".to_string()), Some(item.clone())),
        "mcpToolCall" | "dynamicToolCall" => (
            item_id,
            "tool".to_string(),
            item.get("tool").and_then(Value::as_str).map(str::to_string),
            item.get("arguments").cloned(),
        ),
        "collabAgentToolCall" | "subAgentActivity" => {
            (item_id, "sub_agent".to_string(), Some(kind), Some(item.clone()))
        }
        _ => (item_id, kind.clone(), Some(kind), Some(item.clone())),
    }
}

fn activity_status(item: &Value, fallback: &str) -> String {
    item.get("status")
        .and_then(Value::as_str)
        .map(|status| match status {
            "inProgress" | "running" => "executing",
            "completed" => "success",
            other => other,
        })
        .unwrap_or(fallback)
        .to_string()
}

fn merge_activity_notification(
    items: &mut HashMap<String, Value>,
    method: &str,
    params: &Value,
) -> Option<Value> {
    if matches!(method, "item/started" | "item/completed") {
        let incoming = params.get("item")?.clone();
        let item_id = incoming.get("id")?.as_str()?.to_string();
        if let Some(existing) = items.get_mut(&item_id) {
            if let (Some(existing_object), Some(incoming_object)) =
                (existing.as_object_mut(), incoming.as_object())
            {
                for (key, value) in incoming_object {
                    existing_object.insert(key.clone(), value.clone());
                }
            } else {
                *existing = incoming;
            }
        } else {
            items.insert(item_id.clone(), incoming);
        }
        return items.get(&item_id).cloned();
    }
    if matches!(
        method,
        "item/commandExecution/outputDelta"
            | "item/fileChange/outputDelta"
            | "item/fileChange/patchUpdated"
            | "item/mcpToolCall/progress"
            | "item/plan/delta"
    ) {
        let item_id = params.get("itemId")?.as_str()?.to_string();
        let kind = if method.contains("fileChange") {
            "fileChange"
        } else if method.contains("mcpToolCall") {
            "mcpToolCall"
        } else if method.contains("plan") {
            "plan"
        } else {
            "commandExecution"
        };
        let item = items
            .entry(item_id.clone())
            .or_insert_with(|| json!({"id":item_id,"type":kind}));
        let object = item.as_object_mut()?;
        if let Some(patch) = params.get("patch") {
            object.insert("aggregatedOutput".to_string(), patch.clone());
        } else if let Some(delta) = params
            .get("delta")
            .or_else(|| params.get("message"))
            .and_then(Value::as_str)
        {
            let previous = object
                .get("aggregatedOutput")
                .and_then(Value::as_str)
                .unwrap_or_default();
            object.insert("aggregatedOutput".to_string(), json!(format!("{previous}{delta}")));
        }
        return Some(item.clone());
    }
    None
}

fn generic_item_notification(method: &str, params: &Value) -> Option<Value> {
    if !method.starts_with("item/") {
        return None;
    }
    let item_id = params.get("itemId").and_then(Value::as_str)?;
    Some(json!({
        "id": item_id,
        "type": method.strip_prefix("item/").unwrap_or(method),
        "status": "inProgress",
        "notificationMethod": method,
        "notificationParams": params,
    }))
}

fn codex_turn_usage(params: &Value) -> Option<CodexTurnUsage> {
    let last = params.pointer("/tokenUsage/last")?;
    Some(CodexTurnUsage {
        input_tokens: last.get("inputTokens")?.as_i64()?.try_into().ok()?,
        output_tokens: last.get("outputTokens")?.as_i64()?.try_into().ok()?,
        reasoning_tokens: last.get("reasoningOutputTokens").and_then(Value::as_i64).unwrap_or(0).try_into().ok()?,
        cached_input_tokens: last.get("cachedInputTokens").and_then(Value::as_i64).unwrap_or(0).try_into().ok()?,
        cache_write_input_tokens: last.get("cacheWriteInputTokens").and_then(Value::as_i64).unwrap_or(0).try_into().ok()?,
        total_tokens: last.get("totalTokens")?.as_i64()?.try_into().ok()?,
    })
}

/// 取 item 首次出现时已输出的正文字符数（Unicode 字符计），后续更新沿用同一偏移
fn activity_content_offset(
    offsets: &mut HashMap<String, u64>,
    item: &Value,
    content: &str,
) -> Option<u64> {
    let item_id = item.get("id").and_then(Value::as_str)?.to_string();
    Some(
        *offsets
            .entry(item_id)
            .or_insert_with(|| content.chars().count() as u64),
    )
}

fn emit_activity(
    window: &tauri::Window,
    conversation_id: i64,
    response_message_id: i64,
    thread_id: Option<&str>,
    sequence: u64,
    item: &Value,
    fallback_status: &str,
    content_offset: Option<u64>,
) {
    let (item_id, kind, title, input) = item_identity(item);
    // 用户输入与 agent 正文已经在消息气泡里展示，不再生成活动卡片
    if matches!(kind.as_str(), "userMessage" | "agentMessage") {
        return;
    }
    let activity = AgentActivityEvent {
        conversation_id,
        response_message_id,
        agent_kind: CODEX_APP_SERVER_API_TYPE.to_string(),
        session_id: thread_id.map(str::to_string),
        item_id,
        sequence,
        kind,
        status: activity_status(item, fallback_status),
        title,
        input,
        output: item
            .get("aggregatedOutput")
            .or_else(|| item.get("result"))
            .map(|value| value.as_str().map(str::to_string).unwrap_or_else(|| value.to_string())),
        error: item.get("error").map(Value::to_string),
        metadata: item.clone(),
        content_offset,
    };
    let event = ConversationEvent {
        r#type: "agent_activity".to_string(),
        data: serde_json::to_value(&activity).unwrap(),
    };
    persist_activity(window.app_handle(), response_message_id, &activity);
    let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), event);
}

fn persist_activity(app_handle: &tauri::AppHandle, message_id: i64, activity: &AgentActivityEvent) {
    let Ok(db) = ConversationDatabase::new(app_handle) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(message)) = repo.read(message_id) else { return };
    let mut metadata = message
        .metadata_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let activities = metadata
        .as_object_mut()
        .expect("metadata normalized to object")
        .entry("agent_activities")
        .or_insert_with(|| json!([]));
    let Some(items) = activities.as_array_mut() else { return };
    let replacement = serde_json::to_value(activity).unwrap_or(Value::Null);
    if let Some(existing) = items.iter_mut().find(|candidate| {
        candidate.get("agent_kind") == replacement.get("agent_kind")
            && candidate.get("session_id") == replacement.get("session_id")
            && candidate.get("item_id") == replacement.get("item_id")
    }) {
        if existing.get("sequence").and_then(Value::as_u64).unwrap_or(0) <= activity.sequence {
            *existing = replacement;
        }
    } else {
        items.push(replacement);
    }
    if let Ok(serialized) = serde_json::to_string(&metadata) {
        let _ = repo.update_metadata(message_id, Some(&serialized));
    }
}

fn persist_response(
    app_handle: &tauri::AppHandle,
    message_id: i64,
    content: &str,
    done: bool,
    usage: Option<CodexTurnUsage>,
) {
    if let Ok(db) = ConversationDatabase::new(app_handle) {
        if let Ok(repo) = db.message_repo() {
            let _ = repo.update_content(message_id, content);
            if done {
                if let Ok(Some(mut message)) = repo.read(message_id) {
                    message.finish_time = Some(chrono::Utc::now());
                    message.input_token_count = usage.map(|value| value.input_tokens).unwrap_or(0);
                    message.output_token_count = usage
                        .map(|value| value.output_tokens)
                        .unwrap_or_else(|| ((content.chars().count() + 3) / 4) as i32);
                    message.token_count = usage
                        .map(|value| value.total_tokens)
                        .unwrap_or(message.output_token_count);
                    if let Some(usage) = usage {
                        let mut metadata = message.metadata_json.as_deref()
                            .and_then(|raw| serde_json::from_str::<Value>(raw).ok())
                            .filter(Value::is_object).unwrap_or_else(|| json!({}));
                        if let Some(map) = metadata.as_object_mut() {
                            map.insert("usage_source".to_string(), json!("reported"));
                            map.insert("thought_tokens".to_string(), json!(usage.reasoning_tokens));
                            map.insert("cached_input_tokens".to_string(), json!(usage.cached_input_tokens));
                            map.insert("cached_read_tokens".to_string(), json!(usage.cached_input_tokens));
                            map.insert("cached_write_tokens".to_string(), json!(usage.cache_write_input_tokens));
                            map.insert("cache_creation_tokens".to_string(), json!(usage.cache_write_input_tokens));
                        }
                        message.metadata_json = serde_json::to_string(&metadata).ok();
                    }
                    let _ = repo.update(&message);
                }
            }
        }
    }
}

fn persist_codex_timing(
    app_handle: &tauri::AppHandle,
    message_id: i64,
    start_time: chrono::DateTime<chrono::Utc>,
    first_token_time: Option<chrono::DateTime<chrono::Utc>>,
) {
    let Ok(db) = ConversationDatabase::new(app_handle) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(mut message)) = repo.read(message_id) else { return };
    message.start_time = Some(start_time);
    message.first_token_time = first_token_time;
    message.ttft_ms = first_token_time.map(|first| (first.timestamp_millis() - start_time.timestamp_millis()).max(0));
    let _ = repo.update(&message);
}

fn persist_codex_reasoning(
    app_handle: &tauri::AppHandle,
    message_id: i64,
    content: &str,
) {
    let Ok(db) = ConversationDatabase::new(app_handle) else { return };
    let Ok(repo) = db.message_repo() else { return };
    let Ok(Some(mut message)) = repo.read(message_id) else { return };
    message.content = content.to_string();
    message.finish_time = Some(chrono::Utc::now());
    // Codex reports reasoningOutputTokens as part of the response usage. Keep
    // the separate reasoning bubble informational so conversation totals are
    // not counted twice.
    message.input_token_count = 0;
    message.output_token_count = 0;
    message.token_count = 0;
    let _ = repo.update(&message);
}

async fn spawn_process(config: &CodexAppServerConfig) -> Result<(Child, tokio::process::ChildStdin, Lines<BufReader<ChildStdout>>), String> {
    let resolved_cli = resolve_acp_cli_path(&config.cli_command);
    #[cfg(target_os = "windows")]
    let resolved_cli = if resolved_cli.extension().is_none() {
        let cmd_shim = resolved_cli.with_extension("cmd");
        if cmd_shim.exists() { cmd_shim } else { resolved_cli }
    } else {
        resolved_cli
    };
    #[cfg(target_os = "windows")]
    let (program, prefix_args): (PathBuf, Vec<String>) = match resolved_cli
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("cmd" | "bat") => (
            PathBuf::from("cmd.exe"),
            vec!["/D".to_string(), "/S".to_string(), "/C".to_string(), resolved_cli.display().to_string()],
        ),
        Some("ps1") => (
            PathBuf::from("pwsh.exe"),
            vec!["-NoProfile".to_string(), "-File".to_string(), resolved_cli.display().to_string()],
        ),
        _ => (resolved_cli.clone(), Vec::new()),
    };
    #[cfg(not(target_os = "windows"))]
    let (program, prefix_args): (PathBuf, Vec<String>) = (resolved_cli, Vec::new());

    let mut command = Command::new(&program);
    command
        .args(prefix_args)
        .arg("app-server")
        .arg("--listen")
        .arg("stdio://")
        .args(&config.additional_args)
        .current_dir(&config.working_directory)
        .envs(&config.env_vars)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|error| {
        format!("启动 Codex app-server 失败（{}）：{error}", program.display())
    })?;
    let stdin = child.stdin.take().ok_or("Codex app-server stdin unavailable")?;
    let stdout = child.stdout.take().ok_or("Codex app-server stdout unavailable")?;
    Ok((child, stdin, BufReader::new(stdout).lines()))
}

pub fn spawn_codex_session_task(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    config: CodexAppServerConfig,
) -> CodexSessionHandle {
    let (sender, receiver) = mpsc::unbounded_channel();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = run_session(app_handle.clone(), conversation_id, config, receiver).await {
            error!(conversation_id, error = %error, "Codex app-server session exited");
        }
        if let Some(state) = app_handle.try_state::<crate::CodexSessionState>() {
            state.sessions.lock().await.remove(&conversation_id);
        }
        emit_session_snapshot(&app_handle, conversation_id, None);
    });
    CodexSessionHandle { sender }
}

async fn run_session(
    app_handle: tauri::AppHandle,
    conversation_id: i64,
    config: CodexAppServerConfig,
    mut receiver: mpsc::UnboundedReceiver<CodexSessionCommand>,
) -> Result<(), String> {
    let (_child, mut stdin, mut lines) = spawn_process(&config).await?;
    let mut pending_notifications = VecDeque::new();
    write_frame(
        &mut stdin,
        &json_rpc_request(
            1,
            "initialize",
            json!({
                "clientInfo":{"name":"aipp","title":"AIPP","version":env!("CARGO_PKG_VERSION")},
                "capabilities":{"experimentalApi":true}
            }),
        ),
    )
    .await?;
    read_rpc_response_with_approvals(
        &app_handle,
        conversation_id,
        &mut stdin,
        &mut lines,
        1,
        &mut pending_notifications,
    )
    .await?;
    write_frame(&mut stdin, &json!({"jsonrpc":"2.0","method":"initialized"})).await?;

    let stored_thread = ConversationDatabase::new(&app_handle)
        .ok()
        .and_then(|db| {
            db.get_agent_session_id(conversation_id, CODEX_APP_SERVER_API_TYPE)
                .ok()
                .flatten()
        });
    let mut request_id = 2_u64;
    let mcp_config_overrides = match codex_mcp_bridge_overrides(&app_handle, conversation_id, &config).await {
        Ok(overrides) => overrides,
        Err(error) => {
            warn!(conversation_id, error = %error, "Codex MCP bridge setup failed; continuing without AIPP MCP tools");
            None
        }
    };
    let thread_params = json!({
        "model": config.model,
        "reasoningEffort": config.reasoning_effort,
        "cwd": config.working_directory,
        "approvalPolicy": config.approval_policy,
        "sandbox": config.sandbox,
        "config": mcp_config_overrides,
    });
    let (thread_method, params) = if let Some(thread_id) = stored_thread.as_deref() {
        ("thread/resume", json!({"threadId":thread_id,"config":mcp_config_overrides}))
    } else {
        ("thread/start", thread_params.clone())
    };
    write_frame(&mut stdin, &json_rpc_request(request_id, thread_method, params)).await?;
    let (thread_result, restored_session_method) = match read_rpc_response_with_approvals(
        &app_handle,
        conversation_id,
        &mut stdin,
        &mut lines,
        request_id,
        &mut pending_notifications,
    )
    .await {
        Ok(result) => (
            result,
            stored_thread.as_ref().map(|_| "resume".to_string()),
        ),
        Err(error) if stored_thread.is_some() => {
            warn!(conversation_id, error = %error, "Codex thread resume failed; starting a new thread");
            request_id += 1;
            write_frame(&mut stdin, &json_rpc_request(request_id, "thread/start", thread_params)).await?;
            let result = read_rpc_response_with_approvals(
                &app_handle,
                conversation_id,
                &mut stdin,
                &mut lines,
                request_id,
                &mut pending_notifications,
            )
            .await?;
            (result, None)
        }
        Err(error) => return Err(error),
    };
    let thread_id = thread_id_from_result(&thread_result)
        .ok_or_else(|| format!("Codex thread response missing thread id: {thread_result}"))?;
    request_id += 1;
    let (model_catalog, supported_efforts) = {
        write_frame(&mut stdin, &json_rpc_request(request_id, "model/list", json!({}))).await?;
        match read_rpc_response_with_approvals(
            &app_handle, conversation_id, &mut stdin, &mut lines, request_id, &mut pending_notifications,
        ).await {
            Ok(result) => {
                let catalog = model_catalog(&result);
                let efforts = catalog.iter()
                    .find(|entry| config.model.as_deref() == Some(entry.id.as_str()))
                    .map(|entry| entry.supported_efforts.clone())
                    .unwrap_or_else(|| supported_reasoning_efforts(&result));
                if efforts.is_empty() {
                    warn!(conversation_id, "Codex model/list returned no supportedReasoningEfforts");
                }
                (catalog, efforts)
            }
            Err(error) => {
                warn!(conversation_id, error = %error, "Codex model/list failed; hiding reasoning effort choices");
                (Vec::new(), Vec::new())
            }
        }
    };
    ConversationDatabase::new(&app_handle)
        .map_err(|error| error.to_string())?
        .upsert_agent_session_id(conversation_id, CODEX_APP_SERVER_API_TYPE, &thread_id)
        .map_err(|error| error.to_string())?;
    let mut snapshot = CodexConversationSessionState {
        conversation_id,
        agent_kind: CODEX_APP_SERVER_API_TYPE.to_string(),
        session_id: Some(thread_id.clone()),
        load_session_supported: false,
        session_resume_supported: true,
        restored_session_method,
        current_turn_id: None,
        has_active_prompt: false,
        model: first_string_for_keys(&thread_result, &["model", "modelId"])
            .or_else(|| stored_thread.is_none().then(|| config.model.clone()).flatten()),
        reasoning_effort: first_string_for_keys(&thread_result, &["reasoningEffort", "reasoning_effort"])
            .or_else(|| stored_thread.is_none().then(|| config.reasoning_effort.clone()).flatten()),
        config_options: {
            let mut options = Vec::new();
            {
                options.push(CodexSessionConfigOption {
                    id: "model".to_string(), name: "模型".to_string(),
                    description: Some("Codex 下一轮响应使用的模型；可输入模型 ID".to_string()),
                    category: Some("model".to_string()), current_value: first_string_for_keys(&thread_result, &["model", "modelId"])
                        .or_else(|| stored_thread.is_none().then(|| config.model.clone()).flatten()).unwrap_or_default(),
                    options: codex_model_choices(&model_catalog,
                        first_string_for_keys(&thread_result, &["model", "modelId"])
                            .or_else(|| stored_thread.is_none().then(|| config.model.clone()).flatten())
                            .as_deref()),
                });
            }
            options.push(CodexSessionConfigOption {
            id: "reasoning_effort".to_string(),
            name: "思考强度".to_string(),
            description: Some("Codex 下一轮响应使用的推理强度".to_string()),
            category: Some("thought_level".to_string()),
            current_value: first_string_for_keys(&thread_result, &["reasoningEffort", "reasoning_effort"])
                .or_else(|| stored_thread.is_none().then(|| config.reasoning_effort.clone()).flatten())
                .or_else(|| supported_efforts.first().cloned()).unwrap_or_default(),
            options: codex_reasoning_options(&supported_efforts),
            });
            options
        },
    };
    if let Some(model) = snapshot.model.clone() {
        if let Err(error) = apply_codex_config_option(&mut snapshot, &model_catalog, "model", model) {
            warn!(conversation_id, error = %error, "Codex resumed model is absent from model/list");
        }
    }
    emit_session_snapshot(&app_handle, conversation_id, Some(snapshot.clone()));

    while let Some(command) = receiver.recv().await {
        match command {
            CodexSessionCommand::SetConfigOption { config_id, value, response } => {
                match apply_codex_config_option(&mut snapshot, &model_catalog, &config_id, value) {
                    Ok(()) => {
                        emit_session_snapshot(&app_handle, conversation_id, Some(snapshot.clone()));
                        let _ = response.send(Ok(()));
                    }
                    Err(error) => {
                        let _ = response.send(Err(error));
                    }
                }
            }
            CodexSessionCommand::CancelCurrentPrompt { response } => {
                let _ = response.send(Ok(()));
            }
            CodexSessionCommand::Prompt { message_id, prompt, window } => {
                let turn_start_time = chrono::Utc::now();
                request_id += 1;
                write_frame(
                    &mut stdin,
                    &json_rpc_request(
                        request_id,
                        "turn/start",
                        codex_turn_start_params(&thread_id, &snapshot, &prompt),
                    ),
                )
                .await?;
                let turn_result = read_rpc_response_with_approvals(
                    &app_handle,
                    conversation_id,
                    &mut stdin,
                    &mut lines,
                    request_id,
                    &mut pending_notifications,
                )
                .await?;
                let mut turn_id = turn_result
                    .pointer("/turn/id")
                    .or_else(|| turn_result.get("turnId"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                snapshot.current_turn_id = turn_id.clone();
                snapshot.has_active_prompt = true;
                emit_session_snapshot(&app_handle, conversation_id, Some(snapshot.clone()));
                let mut content = String::new();
                let mut reasoning = String::new();
                let mut reasoning_message_id: Option<i64> = None;
                let mut first_any_token_time: Option<chrono::DateTime<chrono::Utc>> = None;
                let mut turn_usage: Option<CodexTurnUsage> = None;
                let mut turn_error: Option<String> = None;
                let mut sequence = 0_u64;
                let mut activity_items = HashMap::<String, Value>::new();
                // 记录每个 item 开始时的正文字符数，供前端把活动卡片穿插到正文对应位置
                let mut item_content_offsets = HashMap::<String, u64>::new();
                loop {
                    let frame = if let Some(frame) = pending_notifications.pop_front() {
                        frame
                    } else {
                        tokio::select! {
                            maybe_command = receiver.recv() => {
                            match maybe_command {
                                Some(CodexSessionCommand::CancelCurrentPrompt { response }) => {
                                    if let Some(active_turn_id) = turn_id.as_deref() {
                                        request_id += 1;
                                        let result = write_frame(&mut stdin, &json_rpc_request(request_id, "turn/interrupt", json!({"threadId":thread_id,"turnId":active_turn_id}))).await;
                                        let _ = response.send(result);
                                    } else {
                                        let _ = response.send(Ok(()));
                                    }
                                }
                                Some(CodexSessionCommand::Prompt { .. }) => warn!(conversation_id, "Codex prompt received while another turn is active; queued prompt is not supported"),
                                Some(CodexSessionCommand::SetConfigOption { config_id, value, response }) => {
                                    match apply_codex_config_option(&mut snapshot, &model_catalog, &config_id, value) {
                                        Ok(()) => {
                                            emit_session_snapshot(&app_handle, conversation_id, Some(snapshot.clone()));
                                            let _ = response.send(Ok(()));
                                        }
                                        Err(error) => {
                                            let _ = response.send(Err(error));
                                        }
                                    }
                                }
                                None => return Ok(()),
                            }
                            continue;
                            }
                            line = lines.next_line() => {
                            let Some(line) = line.map_err(|error| error.to_string())? else {
                                return Err("Codex app-server closed during turn".to_string());
                            };
                            match serde_json::from_str(&line) {
                                Ok(frame) => frame,
                                Err(error) => { warn!(error = %error, line = %line, "Invalid Codex app-server frame"); continue; }
                            }
                            }
                        }
                    };
                    if frame.get("id").is_some() && frame.get("method").is_some() {
                        if !handle_server_request(
                            &app_handle,
                            conversation_id,
                            &mut stdin,
                            &frame,
                        )
                        .await? {
                            warn!(
                                conversation_id,
                                method = frame.get("method").and_then(|value| value.as_str()).unwrap_or("unknown"),
                                "Unsupported Codex server request"
                            );
                            write_frame(
                                &mut stdin,
                                &json!({
                                    "jsonrpc":"2.0",
                                    "id":frame.get("id").cloned().unwrap_or(Value::Null),
                                    "error":{"code":-32601,"message":format!("AIPP does not support Codex server request {}", frame.get("method").and_then(Value::as_str).unwrap_or("unknown"))}
                                }),
                            )
                            .await?;
                        }
                        continue;
                    }
                    let method = frame.get("method").and_then(Value::as_str).unwrap_or_default();
                    let params = frame.get("params").cloned().unwrap_or(Value::Null);
                    match method {
                                "turn/started" => {
                                    turn_id = params.pointer("/turn/id").and_then(Value::as_str).map(str::to_string).or(turn_id);
                                    snapshot.current_turn_id = turn_id.clone();
                                }
                                "item/agentMessage/delta" => {
                                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                                        if !delta.is_empty() && first_any_token_time.is_none() {
                                            first_any_token_time = Some(chrono::Utc::now());
                                        }
                                        content.push_str(delta);
                                        persist_response(&app_handle, message_id, &content, false, None);
                                        let event = ConversationEvent { r#type: "message_update".to_string(), data: serde_json::to_value(MessageUpdateEvent { message_id, message_type: "response".to_string(), content: content.clone(), is_done: false, token_count: None, input_token_count: None, output_token_count: None, ttft_ms: None, tps: None }).unwrap() };
                                        let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), event);
                                        if let Some(manager) = app_handle.try_state::<ConversationActivityManager>() { manager.set_assistant_streaming(&app_handle, conversation_id, message_id).await; }
                                    }
                                }
                                "item/reasoning/summaryTextDelta" | "item/reasoning/textDelta" => {
                                    if let Some(delta) = params.get("delta").and_then(Value::as_str) {
                                        if !delta.is_empty() && first_any_token_time.is_none() {
                                            first_any_token_time = Some(chrono::Utc::now());
                                        }
                                        reasoning.push_str(delta);
                                        if reasoning_message_id.is_none() {
                                            if let Ok(message) = crate::api::ai_api::add_message(
                                                &app_handle,
                                                Some(message_id),
                                                conversation_id,
                                                "reasoning".to_string(),
                                                String::new(),
                                                Some(0),
                                                Some("codex-app-server".to_string()),
                                                Some(chrono::Utc::now()),
                                                None,
                                                0,
                                                None,
                                                None,
                                            ) {
                                                reasoning_message_id = Some(message.id);
                                                let add_event = ConversationEvent {
                                                    r#type: "message_add".to_string(),
                                                    data: json!({"message_id":message.id,"message_type":"reasoning"}),
                                                };
                                                let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), add_event);
                                            }
                                        }
                                        if let Some(reasoning_id) = reasoning_message_id {
                                            persist_response(&app_handle, reasoning_id, &reasoning, false, None);
                                            let event = ConversationEvent {
                                                r#type: "message_update".to_string(),
                                                data: serde_json::to_value(MessageUpdateEvent {
                                                    message_id: reasoning_id,
                                                    message_type: "reasoning".to_string(),
                                                    content: reasoning.clone(),
                                                    is_done: false,
                                                    token_count: None,
                                                    input_token_count: None,
                                                    output_token_count: None,
                                                    ttft_ms: None,
                                                    tps: None,
                                                }).unwrap(),
                                            };
                                            let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), event);
                                        }
                                    }
                                }
                                "item/started" | "item/completed" => {
                                    sequence += 1;
                                    if let Some(item) = merge_activity_notification(&mut activity_items, method, &params) {
                                        let offset = activity_content_offset(&mut item_content_offsets, &item, &content);
                                        emit_activity(&window, conversation_id, message_id, Some(&thread_id), sequence, &item, if method == "item/completed" { "success" } else { "executing" }, offset);
                                    }
                                }
                                "item/commandExecution/outputDelta" | "item/fileChange/outputDelta" | "item/fileChange/patchUpdated" | "item/mcpToolCall/progress" | "item/plan/delta" => {
                                    sequence += 1;
                                    if let Some(item) = merge_activity_notification(&mut activity_items, method, &params) {
                                        let offset = activity_content_offset(&mut item_content_offsets, &item, &content);
                                        emit_activity(&window, conversation_id, message_id, Some(&thread_id), sequence, &item, "executing", offset);
                                    }
                                }
                                "thread/tokenUsage/updated" => {
                                    turn_usage = codex_turn_usage(&params).or(turn_usage);
                                }
                                "turn/completed" => {
                                    let turn = params.get("turn").unwrap_or(&params);
                                    if turn.get("status").and_then(Value::as_str) == Some("failed") {
                                        turn_error = turn
                                            .pointer("/error/message")
                                            .and_then(Value::as_str)
                                            .map(str::to_string)
                                            .or_else(|| Some("Codex turn failed".to_string()));
                                    }
                                    break;
                                },
                                "mcpServer/startupStatus/updated" => {
                                    let server_name = params.get("name").and_then(Value::as_str).unwrap_or("unknown");
                                    let status = params.get("status").and_then(Value::as_str).unwrap_or("unknown");
                                    if status == "failed" {
                                        warn!(conversation_id, server = server_name, error = ?params.get("error"), failure_reason = ?params.get("failureReason"), "Codex MCP server startup failed");
                                    } else {
                                        debug!(conversation_id, server = server_name, status, "Codex MCP server startup status");
                                    }
                                }
                                "error" => {
                                    let message = params.get("message").and_then(Value::as_str).unwrap_or("Codex app-server error");
                                    return Err(message.to_string());
                                }
                                _ => {
                                    if let Some(item) = generic_item_notification(method, &params) {
                                        sequence += 1;
                                        let offset = activity_content_offset(&mut item_content_offsets, &item, &content);
                                        emit_activity(&window, conversation_id, message_id, Some(&thread_id), sequence, &item, "executing", offset);
                                    } else if !method.is_empty() {
                                        warn!(conversation_id, method, "Unhandled Codex notification");
                                    }
                                }
                    }
                    }
                persist_codex_timing(&app_handle, message_id, turn_start_time, first_any_token_time);
                persist_response(&app_handle, message_id, &content, true, turn_usage);
                if let Some(reasoning_id) = reasoning_message_id {
                    persist_codex_reasoning(&app_handle, reasoning_id, &reasoning);
                    let _ = window.emit(
                        format!("conversation_event_{conversation_id}").as_str(),
                        ConversationEvent {
                            r#type: "message_update".to_string(),
                            data: serde_json::to_value(MessageUpdateEvent {
                                message_id: reasoning_id,
                                message_type: "reasoning".to_string(),
                                content: reasoning.clone(),
                                is_done: true,
                                token_count: Some(0),
                                input_token_count: None,
                                output_token_count: Some(0),
                                ttft_ms: None,
                                tps: None,
                            }).unwrap(),
                        },
                    );
                }
                let estimated_output_tokens = ((content.chars().count() + 3) / 4) as i32;
                let finish_time = chrono::Utc::now();
                let ttft_ms = first_any_token_time.map(|first| (first.timestamp_millis() - turn_start_time.timestamp_millis()).max(0));
                let output_tokens = turn_usage.map(|usage| usage.output_tokens).unwrap_or(estimated_output_tokens);
                let tps = first_any_token_time.and_then(|first| {
                    let duration_ms = finish_time.timestamp_millis() - first.timestamp_millis();
                    (output_tokens > 0 && duration_ms > 0).then(|| output_tokens as f64 * 1000.0 / duration_ms as f64)
                });
                let done_event = ConversationEvent { r#type: "message_update".to_string(), data: serde_json::to_value(MessageUpdateEvent { message_id, message_type: "response".to_string(), content: content.clone(), is_done: true, token_count: Some(turn_usage.map(|usage| usage.total_tokens).unwrap_or(estimated_output_tokens)), input_token_count: turn_usage.map(|usage| usage.input_tokens), output_token_count: Some(output_tokens), ttft_ms, tps }).unwrap() };
                let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), done_event);
                let complete_event = ConversationEvent { r#type: "stream_complete".to_string(), data: json!({"conversation_id":conversation_id,"response_message_id":message_id,"reasoning_message_id":reasoning_message_id,"has_response":!content.is_empty(),"has_reasoning":!reasoning.is_empty(),"response_length":content.len(),"reasoning_length":reasoning.len()}) };
                let _ = window.emit(format!("conversation_event_{conversation_id}").as_str(), complete_event);
                if let Some(manager) = app_handle.try_state::<ConversationActivityManager>() { manager.clear_focus(&app_handle, conversation_id).await; }
                snapshot.current_turn_id = None;
                snapshot.has_active_prompt = false;
                emit_session_snapshot(&app_handle, conversation_id, Some(snapshot.clone()));
                if let Some(error) = turn_error {
                    return Err(error);
                }
            }
        }
    }
    info!(conversation_id, "Codex app-server session command channel closed");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_thread_id_from_v2_response() {
        assert_eq!(thread_id_from_result(&json!({"thread":{"id":"thread-1"}})).as_deref(), Some("thread-1"));
    }

    #[test]
    fn maps_command_item_without_numeric_id() {
        let item = json!({"type":"commandExecution","id":"item-abc","command":"cargo check","cwd":"C:/repo","status":"inProgress"});
        let (id, kind, title, input) = item_identity(&item);
        assert_eq!(id, "item-abc");
        assert_eq!(kind, "command");
        assert_eq!(title.as_deref(), Some("cargo check"));
        assert_eq!(input.unwrap()["cwd"], "C:/repo");
        assert_eq!(activity_status(&item, "pending"), "executing");
    }

    /// item 首次出现时记录当前正文字符数，后续更新沿用同一偏移
    #[test]
    fn pins_content_offset_at_first_seen() {
        let mut offsets = HashMap::new();
        let item = json!({"id":"item-1","type":"commandExecution"});
        let first = activity_content_offset(&mut offsets, &item, "你好");
        assert_eq!(first, Some(2));
        // 正文继续增长后再次取偏移，仍返回首次记录的值
        let second = activity_content_offset(&mut offsets, &item, "你好世界");
        assert_eq!(second, Some(2));
        // 另一个 item 记录自己的偏移
        let other = activity_content_offset(&mut offsets, &json!({"id":"item-2","type":"commandExecution"}), "你好世界");
        assert_eq!(other, Some(4));
        // 没有 id 的 item 拿不到偏移
        assert_eq!(activity_content_offset(&mut offsets, &json!({"type":"commandExecution"}), "你好世界"), None);
    }

    #[test]
    fn maps_codex_approval_decisions_to_protocol_responses() {
        let params = json!({"availableDecisions":["accept","decline"]});
        let options = codex_permission_options("item/commandExecution/requestApproval", &params);
        assert_eq!(options.iter().map(|item| item.option_id.as_str()).collect::<Vec<_>>(), vec!["accept", "decline"]);
        assert_eq!(
            codex_approval_response(
                "item/commandExecution/requestApproval",
                &params,
                AcpPermissionDecision::Selected("accept".to_string())
            ),
            json!({"decision":"accept"})
        );
        assert_eq!(
            codex_approval_response(
                "item/permissions/requestApproval",
                &json!({"permissions":{"fileSystem":{"entries":[]}}}),
                AcpPermissionDecision::Selected("acceptForSession".to_string())
            )["scope"],
            "session"
        );
    }

    #[test]
    fn parses_model_catalog_and_switches_reasoning_capabilities() {
        let catalog = model_catalog(&json!({"data":[
            {"id":"gpt-a","displayName":"GPT A","supportedReasoningEfforts":[{"effort":"low"},{"effort":"high"}],"defaultReasoningEffort":"low"},
            {"id":"gpt-b","displayName":"GPT B","supportedReasoningEfforts":["none","medium"]}
        ]}));
        assert_eq!(catalog.len(), 2);
        let mut snapshot = CodexConversationSessionState {
            model: Some("gpt-a".to_string()), reasoning_effort: Some("high".to_string()), ..Default::default()
        };
        snapshot.config_options = vec![
            CodexSessionConfigOption { id: "model".to_string(), name: "模型".to_string(), description: None, category: Some("model".to_string()), current_value: "gpt-a".to_string(), options: vec![] },
            CodexSessionConfigOption { id: "reasoning_effort".to_string(), name: "思考强度".to_string(), description: None, category: Some("thought_level".to_string()), current_value: "high".to_string(), options: vec![] },
        ];
        apply_codex_config_option(&mut snapshot, &catalog, "model", "gpt-b".to_string()).unwrap();
        assert_eq!(snapshot.model.as_deref(), Some("gpt-b"));
        assert_eq!(snapshot.reasoning_effort.as_deref(), Some("none"));
        assert_eq!(snapshot.config_options[1].options.len(), 2);
    }

    #[test]
    fn preserves_resumed_model_when_catalog_does_not_contain_it() {
        let choices = codex_model_choices(&[], Some("retired-model"));
        assert_eq!(choices[0].value, "retired-model");
        assert!(choices[0].name.contains("当前线程"));
    }

    #[test]
    fn builds_turn_start_with_runtime_snapshot_values() {
        let snapshot = CodexConversationSessionState { model: Some("gpt-a".to_string()), reasoning_effort: Some("ultra".to_string()), ..Default::default() };
        assert_eq!(codex_turn_start_params("thread-1", &snapshot, "hello")["model"], "gpt-a");
        assert_eq!(codex_turn_start_params("thread-1", &snapshot, "hello")["reasoningEffort"], "ultra");
    }

    #[test]
    fn maps_codex_request_user_input_to_aipp_and_back() {
        let params = json!({
            "questions": [
                {
                    "id": "scope",
                    "header": "范围",
                    "question": "选择范围",
                    "options": [
                        {"label": "当前文件", "description": "只处理当前文件"},
                        {"label": "全部文件", "description": "处理所有文件"}
                    ]
                },
                {
                    "id": "note",
                    "header": "备注",
                    "question": "补充说明",
                    "options": null,
                    "isSecret": true
                }
            ]
        });
        let (request, mappings) = codex_user_input_request(&params).unwrap();
        assert_eq!(request.questions.len(), 2);
        assert!(request.questions[1].options.is_empty());
        assert!(request.questions[1].is_secret);

        let answers = HashMap::from([
            ("选择范围".to_string(), "全部文件".to_string()),
            ("补充说明".to_string(), "仅测试".to_string()),
        ]);
        let response = codex_user_input_response(&mappings, &answers);
        assert_eq!(response["answers"]["scope"]["answers"], json!(["全部文件"]));
        assert_eq!(response["answers"]["note"]["answers"], json!(["仅测试"]));
    }

    #[test]
    fn reads_exact_turn_usage_and_degrades_unknown_item_notification() {
        let usage = codex_turn_usage(&json!({
            "tokenUsage": {
                "last": {
                    "inputTokens": 12,
                    "outputTokens": 7,
                    "reasoningOutputTokens": 3,
                    "cachedInputTokens": 5,
                    "cacheWriteInputTokens": 2,
                    "totalTokens": 19
                }
            }
        }));
        assert_eq!(
            usage,
            Some(CodexTurnUsage {
                input_tokens: 12,
                output_tokens: 7,
                reasoning_tokens: 3,
                cached_input_tokens: 5,
                cache_write_input_tokens: 2,
                total_tokens: 19,
            })
        );

        let item = generic_item_notification(
            "item/futureFeature/progress",
            &json!({"itemId":"future-1","value":42}),
        )
        .unwrap();
        assert_eq!(item["id"], "future-1");
        assert_eq!(item["type"], "futureFeature/progress");
    }

    #[test]
    fn merges_command_output_deltas_by_string_item_id() {
        let mut items = HashMap::new();
        let first = merge_activity_notification(
            &mut items,
            "item/commandExecution/outputDelta",
            &json!({"itemId":"item-1","delta":"one\n"}),
        )
        .unwrap();
        let second = merge_activity_notification(
            &mut items,
            "item/commandExecution/outputDelta",
            &json!({"itemId":"item-1","delta":"two"}),
        )
        .unwrap();
        assert_eq!(first["aggregatedOutput"], "one\n");
        assert_eq!(second["aggregatedOutput"], "one\ntwo");
    }

    fn model_config(name: &str, value: &str) -> crate::db::assistant_db::AssistantModelConfig {
        crate::db::assistant_db::AssistantModelConfig {
            id: 0,
            assistant_id: 0,
            assistant_model_id: 0,
            name: name.to_string(),
            value: Some(value.to_string()),
            value_type: "string".to_string(),
        }
    }

    fn provider_config(name: &str, value: &str) -> crate::db::llm_db::LLMProviderConfig {
        crate::db::llm_db::LLMProviderConfig {
            id: 0,
            name: name.to_string(),
            llm_provider_id: 0,
            value: value.to_string(),
            append_location: String::new(),
            is_addition: false,
        }
    }

    /// 助手级配置（assistant_model_config）应优先于 provider 默认值
    ///
    /// 验证内容：
    /// - 工作目录/附加启动参数：助手级覆盖 provider 级
    /// - 环境变量：provider 表单 JSON 格式与助手级 KEY=VALUE 行格式均可解析
    /// - 同名环境变量助手级优先
    #[test]
    fn assistant_level_config_overrides_provider_defaults() {
        let model_configs = vec![
            model_config("acp_working_directory", "/tmp/assistant-cwd"),
            model_config("acp_additional_args", "--enable collaboration_modes"),
            model_config("acp_env_vars", "ASSISTANT_KEY=assistant\nSHARED_KEY=from-assistant"),
        ];
        let provider_configs = vec![
            provider_config("acp_working_directory", "/tmp/provider-cwd"),
            provider_config("codex_additional_args", "--profile provider"),
            provider_config(
                "acp_env_vars",
                "{\"SHARED_KEY\":\"from-provider\",\"PROVIDER_KEY\":\"provider\"}",
            ),
        ];
        let config = extract_codex_app_server_config(&model_configs, &provider_configs, None).unwrap();
        assert_eq!(config.working_directory, PathBuf::from("/tmp/assistant-cwd"));
        assert_eq!(
            config.additional_args,
            vec!["--enable".to_string(), "collaboration_modes".to_string()]
        );
        assert_eq!(config.env_vars.get("ASSISTANT_KEY").map(String::as_str), Some("assistant"));
        assert_eq!(config.env_vars.get("SHARED_KEY").map(String::as_str), Some("from-assistant"));
        assert_eq!(config.env_vars.get("PROVIDER_KEY").map(String::as_str), Some("provider"));
    }

    /// 未设置助手级覆盖时回退到 provider 默认值
    #[test]
    fn provider_defaults_apply_without_assistant_overrides() {
        let provider_configs = vec![
            provider_config("acp_working_directory", "/tmp/provider-cwd"),
            provider_config("codex_additional_args", "--profile provider"),
        ];
        let config = extract_codex_app_server_config(&[], &provider_configs, None).unwrap();
        assert_eq!(config.working_directory, PathBuf::from("/tmp/provider-cwd"));
        assert_eq!(
            config.additional_args,
            vec!["--profile".to_string(), "provider".to_string()]
        );
    }

    fn codex_test_config() -> CodexAppServerConfig {
        CodexAppServerConfig {
            cli_command: "codex".to_string(),
            working_directory: PathBuf::from("/tmp/work"),
            env_vars: HashMap::new(),
            additional_args: Vec::new(),
            model: None,
            reasoning_effort: None,
            approval_policy: None,
            sandbox: None,
            selected_mcp_tools_payload: String::new(),
            session_signature: String::new(),
        }
    }

    /// MCP 工具快照纳入签名：绑定变化会触发会话重建
    #[test]
    fn signature_changes_with_selected_mcp_payload() {
        let mut config = codex_test_config();
        config.session_signature = codex_config_signature(&config);
        let without_mcp = config.session_signature.clone();
        config.selected_mcp_tools_payload = "[{\"server_id\":1}]".to_string();
        refresh_codex_session_signature(&mut config);
        assert_ne!(without_mcp, config.session_signature);
    }

    /// 构造的 config 覆盖包含 aipp server 的命令、桥接参数与全部环境变量
    #[test]
    fn build_codex_mcp_config_overrides_contains_bridge_env() {
        let proxy = AcpMcpProxyConfig {
            addr: "127.0.0.1:1234".to_string(),
            token: "tok".to_string(),
        };
        let overrides = build_codex_mcp_config_overrides(
            std::path::Path::new("aipp.exe"),
            std::path::Path::new("mcp.db"),
            42,
            &proxy,
            "[{\"server_id\":1}]",
        );
        assert_eq!(
            overrides.get("mcp_servers.aipp.command").and_then(Value::as_str),
            Some("aipp.exe")
        );
        assert_eq!(
            overrides.get("mcp_servers.aipp.args"),
            Some(&json!([ACP_MCP_BRIDGE_ARG]))
        );
        assert_eq!(
            overrides.get("mcp_servers.aipp.startup_timeout_sec"),
            Some(&json!(20))
        );
        let env = overrides
            .get("mcp_servers.aipp.env")
            .and_then(Value::as_object)
            .unwrap();
        for key in [
            ACP_MCP_DB_PATH_ENV,
            ACP_MCP_CONVERSATION_ID_ENV,
            ACP_MCP_NATIVE_DUPLICATE_FILTER_ENV,
            ACP_MCP_PROXY_ADDR_ENV,
            ACP_MCP_PROXY_TOKEN_ENV,
            ACP_MCP_SELECTED_TOOLS_ENV,
        ] {
            assert!(env.contains_key(key), "missing env {key}");
        }
        assert_eq!(
            env.get(ACP_MCP_CONVERSATION_ID_ENV).and_then(Value::as_str),
            Some("42")
        );
        assert_eq!(
            env.get(ACP_MCP_PROXY_ADDR_ENV).and_then(Value::as_str),
            Some("127.0.0.1:1234")
        );
        assert_eq!(
            env.get(ACP_MCP_SELECTED_TOOLS_ENV).and_then(Value::as_str),
            Some("[{\"server_id\":1}]")
        );
    }
}
