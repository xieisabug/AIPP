use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, Window};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tracing::{debug, warn};

use crate::api::ai::acp::AcpPermissionState;
use crate::api::ai::events::ConversationRuntimeState;
use crate::api::ai::types::AiRequest;
use crate::api::ai_api::{add_message, ask_ai};
use crate::api::assistant_summary_api::build_butler_assistant_directory_prompt;
use crate::db::assistant_db::{Assistant, AssistantDatabase};
use crate::db::conversation_db::{
    ButlerTaskDefinition, ButlerTaskResult, Conversation, ConversationDatabase, Message, Repository,
};
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::{MCPDatabase, MCPServer, MCPServerTool};
use crate::db::skill_db::SkillDatabase;
use crate::feishu::inherit_latest_feishu_target;
use crate::mcp::builtin_mcp::OperationState;
use crate::mcp::registry_api::ensure_agent_load_skill_for_assistant;
use crate::skills::scanner::SkillScanner;

const EXPERIMENTAL_FEATURE_CODE: &str = "experimental";
const BUTLER_MAIN_SLOT: &str = "default";
const BUTLER_KIND_MAIN: &str = "butler_main";
const BUTLER_KIND_MAIN_ARCHIVE: &str = "butler_main_archive";
const BUTLER_KIND_TASK: &str = "butler_task";
const STATUS_ACCEPTED: &str = "accepted";
const STATUS_RUNNING: &str = "running";
const STATUS_SUCCEEDED: &str = "succeeded";
const STATUS_FAILED: &str = "failed";
const STATUS_CANCELLED: &str = "cancelled";
pub(crate) const BUTLER_SYSTEM_ASSISTANT_NAME: &str = "__aipp_internal_butler_system_assistant__";
const BUTLER_SYSTEM_ASSISTANT_DESCRIPTION: &str = "AIPP 总管家系统保留助手，请勿展示给普通用户。";
const TASK_RESULT_DETAIL_LIMIT: usize = 4000;
type ButlerContinuationLockRegistry = Arc<Mutex<HashMap<i64, Arc<Mutex<()>>>>>;
static BUTLER_MAIN_CONTINUATION_LOCKS: OnceLock<ButlerContinuationLockRegistry> = OnceLock::new();
static BUTLER_TASK_FINALIZATION_LOCKS: OnceLock<ButlerContinuationLockRegistry> = OnceLock::new();
const BUTLER_TASK_WATCHER_POLL_INTERVAL: Duration = Duration::from_secs(2);
const BUTLER_TASK_WATCHER_IDLE_OBSERVATIONS: usize = 2;
const BUTLER_TASK_RECONCILE_INTERVAL: Duration = Duration::from_secs(60);
const FOLLOWUP_STATUS_PENDING: &str = "pending";
const FOLLOWUP_STATUS_DISPATCHING: &str = "dispatching";
const FOLLOWUP_STATUS_HANDOFF_INJECTED: &str = "handoff_injected";
const FOLLOWUP_STATUS_ENQUEUED: &str = "enqueued";
const BUTLER_SYSTEM_PROMPT_BASE: &str = r#"你是 AIPP 的总管家，是负责理解目标、拆解任务、选择执行助手、派发子任务、汇总结果并给出建议的内置系统角色，你的核心职责是总控、调度、判断和汇总。

工作原则：
1. 先理解用户真正目标，计划完成该任务所需的步骤并且调用todo工具记录；信息不足时先澄清。
2. 只有在问题非常小、无需独立执行上下文时，才直接回答；凡是实现、修复、调研、撰写、整理、验证、运行这类可交付工作，默认应派发为子任务。
3. 派发任务时，尽量写清任务标题、目标、约束、交付物和验收标准。
4. 优先显式指定执行助手，不要把执行人选择留空。
5. 子任务必须通过新的任务对话来驱动执行助手完成；你要显式使用 `spawn_task_conversation` 创建新对话，而不是自己冒充执行助手展开长流程。
6. 子任务是独立执行单元；你负责跟踪状态、读取结果、做比较和汇总，不假装自己完成了子任务。
7. 汇总结果时，清楚标注来源，区分“已经确认的结论”和“仍需补充的信息”。
8. 面对可能影响外部世界的行为时保持谨慎；如果系统要求审批或确认，遵循运行时约束，不要绕过。
9. 系统会在子任务进入终态后，通过 `<butler_task_result>` 任务回流消息把结果重新送回你；收到后要把它视为内部执行回调，立即决定下一步，而不是等待用户再次催促。
10. 当系统通过 `<butler_task_attention>` 提醒某个子任务出现权限请求、等待确认或其他阻塞时，你应优先使用 `task_conversation_operation` 查看该 task conversation 最新一条或少量最新消息，并直接执行确认或补充提示，而不是被动等待人工处理。
11. 阅读子任务上下文时默认最小化：先看最新 1 条消息和当前待处理权限，确有必要再扩大范围，不要把整个子任务历史一次性搬进主会话。
12. 如果 `task_conversation_operation read` 返回的待处理权限里包含 `butler_review.manual_review_required=true`，你不得直接确认该请求，只能等待用户在桌面端或飞书端人工审核；典型例子包括带有 `rm`/删除类命令的请求。
13. 即使没有 `manual_review_required=true`，你也必须保持安全优先：默认选择最小授权，不要自动使用 `allow_and_save`，也不要选择 ACP 的持久授权选项（如 `allow_always`）。
14. 不可以产出非要求格式的结果，比如要求交互展示却只生成了代码让用户去手动执行（正确的做法应该是生成html文件或者Artifact），比如要求交付office文档格式Word、Excel、PowerPoint 却只输出了Markdown或者代码块（应该使用skills或者代码执行能力生成对应的文件），如果实在无法产出对应的文件，应该与用户确认后再生成其他降级的格式。
15. 当 `<butler_task_attention>` 的 attention_kind 为 `ask_user_question` 时，说明子任务助手正在向你提问并等待回答。你应先使用 `task_conversation_operation read` 查看 `pending_ask_user_questions` 列表，理解问题内容后，使用 `task_conversation_operation` 的 `ask_user_respond` action 提供回答。回答应基于你已有的任务上下文和对话历史做出合理判断；如果确实无法判断，可以将问题转述给用户。

能力使用规则：
1. 系统会先在上下文中注入可派发助手目录，再注入当前可用的 MCP 工具与 Skills 目录，把它们当作运行时能力目录来使用。
2. 仅基于当前上下文中实际注入的能力做决策，不要假设未注入的工具、技能或权限已经存在。
3. 派工时优先参考“可派发助手目录”，按任务目标、能力匹配、MCP/Skills 依赖和产出风格来选择执行助手。
4. 若用户要求“做、实现、修复、调研、整理、产出、推进”，优先先拆成子任务，再显式使用 `spawn_task_conversation` 创建新的任务对话。
5. `spawn_task_conversation` 与 `task_conversation_operation` 是你推进子任务的主要机制：前者用于创建任务，后者用于查看和操作已存在的任务会话。
6. 若任务更适合交给专门助手、专门工具链或独立上下文执行，优先拆解并派发。
7. 必要时，可以加载 AIPP相关的 skills 来增强完成任务的能力，这些 skills 会让你对整个系统有更清晰的认识。
8. 如果有类似准备资料、产出文件的任务，你可以使用文件系统来进行辅助，也可使用文件系统来辅助多个助手任务之间的协作。

沟通风格：
- 直接、有判断、不说空话。
- 先给结论，再给结构化说明。
- 面向负责人视角表达：关注进度、风险、依赖、备选方案和下一步。
- 不做表演式热情，不用空洞套话。

结果归属规则：
- 可以说“根据任务 A 的结果”或“执行助手 B 返回了如下结论”。
- 不要把子任务产出描述成你亲自完成的工作。

你的目标不是看起来忙，而是稳定地把事情推进到有结果、有判断、有下一步。

当前的日期是 !cd
"#;

#[derive(Debug, Clone)]
pub(crate) struct ButlerModelSelection {
    pub(crate) raw_value: String,
    pub(crate) model_code: String,
    pub(crate) provider_id: i64,
    pub(crate) display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButlerTaskListItem {
    pub butler_conversation_id: i64,
    pub task_conversation_id: i64,
    pub title: String,
    pub goal: String,
    pub status: String,
    pub executor_assistant_id: i64,
    pub executor_assistant_name: String,
    pub last_summary: Option<String>,
    pub created_time: DateTime<Utc>,
    pub updated_time: DateTime<Utc>,
    pub finalized_at: Option<DateTime<Utc>>,
    pub is_finalized: bool,
    pub has_pending_permission: bool,
    pub is_running: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButlerTaskDetailResponse {
    pub task: ButlerTaskListItem,
    pub conversation: Conversation,
    pub definition: ButlerTaskDefinition,
    pub result: Option<ButlerTaskResult>,
    pub runtime_state: ConversationRuntimeState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButlerMainLoadResponse {
    pub conversation: Conversation,
    pub model_id: String,
    pub model_display_name: String,
    pub tasks: Vec<ButlerTaskListItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnButlerTaskRequest {
    pub butler_conversation_id: i64,
    pub title: String,
    pub goal: String,
    pub executor_assistant_id: Option<i64>,
    pub executor_assistant_name: Option<String>,
    pub handoff_contract_json: Option<String>,
    pub result_handling_mode: Option<String>,
    pub notification_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnButlerTaskResponse {
    pub butler_conversation_id: i64,
    pub task_conversation_id: i64,
    pub title: String,
    pub status: String,
    pub executor_assistant_id: i64,
    pub executor_assistant_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButlerTaskResultAvailableEvent {
    pub task: ButlerTaskListItem,
    pub result: ButlerTaskResult,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButlerNotificationEvent {
    pub butler_conversation_id: i64,
    pub task_conversation_id: i64,
    pub notification_type: String,
    pub title: String,
    pub body: String,
    pub importance: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ButlerTaskTerminalDecision {
    status: &'static str,
    summary: String,
    detail_text: String,
    final_message_id: Option<i64>,
    cancel_requested: bool,
}

fn butler_main_continuation_lock_registry() -> &'static ButlerContinuationLockRegistry {
    BUTLER_MAIN_CONTINUATION_LOCKS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

fn butler_task_finalization_lock_registry() -> &'static ButlerContinuationLockRegistry {
    BUTLER_TASK_FINALIZATION_LOCKS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

pub(crate) async fn get_butler_main_continuation_lock(conversation_id: i64) -> Arc<Mutex<()>> {
    let registry = butler_main_continuation_lock_registry();
    let mut guard = registry.lock().await;
    guard.entry(conversation_id).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

async fn get_butler_task_finalization_lock(conversation_id: i64) -> Arc<Mutex<()>> {
    let registry = butler_task_finalization_lock_registry();
    let mut guard = registry.lock().await;
    guard.entry(conversation_id).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
}

fn parse_bool_flag(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    normalized == "true" || normalized == "1" || normalized == "yes" || normalized == "on"
}

pub(crate) fn is_butler_system_assistant_name(name: &str) -> bool {
    name.trim() == BUTLER_SYSTEM_ASSISTANT_NAME
}

pub(crate) fn is_butler_system_assistant(assistant: &Assistant) -> bool {
    is_butler_system_assistant_name(&assistant.name)
}

async fn get_experimental_config_value(app_handle: &AppHandle, key: &str) -> Option<String> {
    let feature_state = app_handle.state::<crate::FeatureConfigState>();
    let config_map = feature_state.config_feature_map.lock().await;
    config_map
        .get(EXPERIMENTAL_FEATURE_CODE)
        .and_then(|feature_map| feature_map.get(key))
        .map(|config| config.value.clone())
}

async fn ensure_butler_enabled(app_handle: &AppHandle) -> Result<(), String> {
    let enabled = get_experimental_config_value(app_handle, "butler_experiment_enabled")
        .await
        .map(|value| parse_bool_flag(&value))
        .unwrap_or(false);
    if enabled {
        Ok(())
    } else {
        Err("请先在实验性功能中启用总管家实验模式".to_string())
    }
}

async fn build_butler_system_prompt(app_handle: &AppHandle) -> Result<String, String> {
    let assistant_directory_prompt = build_butler_assistant_directory_prompt(app_handle).await?;
    Ok(format!("{}\n\n{}", BUTLER_SYSTEM_PROMPT_BASE, assistant_directory_prompt))
}

pub(crate) async fn get_butler_model_selection(
    app_handle: &AppHandle,
) -> Result<ButlerModelSelection, String> {
    let raw_value = get_experimental_config_value(app_handle, "butler_model_id")
        .await
        .ok_or_else(|| "请先在实验性功能中为总管家选择模型".to_string())?;
    let raw_value = raw_value.trim().to_string();
    if raw_value.is_empty() {
        return Err("请先在实验性功能中为总管家选择模型".to_string());
    }
    let parts: Vec<&str> = raw_value.split("%%").collect();
    if parts.len() != 2 {
        return Err("总管家模型配置无效，请重新选择".to_string());
    }
    let model_code = parts[0].trim().to_string();
    let provider_id =
        parts[1].trim().parse::<i64>().map_err(|_| "总管家模型配置无效，请重新选择".to_string())?;
    if model_code.is_empty() {
        return Err("总管家模型配置无效，请重新选择".to_string());
    }

    let llm_db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let model_detail = llm_db
        .get_llm_model_detail(&provider_id, &model_code)
        .map_err(|e| format!("无法读取总管家模型配置: {}", e))?;

    Ok(ButlerModelSelection {
        raw_value,
        model_code,
        provider_id,
        display_name: model_detail.model.name,
    })
}

fn visible_executor_candidates(assistants: Vec<Assistant>) -> Vec<Assistant> {
    assistants.into_iter().filter(|assistant| !is_butler_system_assistant(assistant)).collect()
}

fn resolve_default_executor_assistant(
    assistant_db: &AssistantDatabase,
) -> Result<Assistant, String> {
    let assistants =
        visible_executor_candidates(assistant_db.get_assistants().map_err(|e| e.to_string())?);
    assistants
        .iter()
        .find(|assistant| assistant.id != 1)
        .cloned()
        .or_else(|| assistants.into_iter().next())
        .ok_or_else(|| "当前没有可用于执行任务的助手，请先创建至少一个普通助手".to_string())
}

fn should_force_auto_run_for_butler_tool(_server: &MCPServer, _tool: &MCPServerTool) -> bool {
    true
}

fn sync_butler_mcp_capabilities(app_handle: &AppHandle, assistant_id: i64) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let mcp_db = MCPDatabase::new(app_handle).map_err(|e| e.to_string())?;

    for server in mcp_db.get_mcp_servers().map_err(|e| e.to_string())? {
        assistant_db
            .upsert_assistant_mcp_config(assistant_id, server.id, server.is_enabled)
            .map_err(|e| e.to_string())?;

        for tool in mcp_db.get_mcp_server_tools(server.id).map_err(|e| e.to_string())?.into_iter() {
            let is_enabled = server.is_enabled && tool.is_enabled;
            let auto_run = is_enabled && should_force_auto_run_for_butler_tool(&server, &tool);
            assistant_db
                .upsert_assistant_mcp_tool_config(assistant_id, tool.id, is_enabled, auto_run)
                .map_err(|e| e.to_string())?;
        }
    }

    Ok(())
}

fn sync_butler_skill_capabilities(app_handle: &AppHandle, assistant_id: i64) -> Result<(), String> {
    let skill_db = SkillDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let home_dir = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let app_data_dir =
        app_handle.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let scanner = SkillScanner::new(home_dir, app_data_dir);
    let mut scanned_skills = scanner.scan_all();
    scanned_skills.sort_by(|left, right| left.identifier.cmp(&right.identifier));

    for (priority, skill) in scanned_skills.iter().enumerate() {
        skill_db
            .upsert_assistant_skill_config(assistant_id, &skill.identifier, true, priority as i32)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

fn is_external_channel_metadata_system_message(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with("<external_channel_input>")
        && trimmed.ends_with("</external_channel_input>")
}

fn cleanup_butler_external_channel_system_messages(
    app_handle: &AppHandle,
    conversation_id: i64,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let message_repo = db.message_repo().map_err(|e| e.to_string())?;
    let polluted_message_ids: Vec<i64> = message_repo
        .list_by_conversation_id(conversation_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter_map(|(message, _)| {
            (message.message_type == "system"
                && is_external_channel_metadata_system_message(&message.content))
            .then_some(message.id)
        })
        .collect();

    for message_id in polluted_message_ids {
        message_repo.delete(message_id).map_err(|e| e.to_string())?;
    }

    Ok(())
}

async fn ensure_butler_system_assistant(app_handle: &AppHandle) -> Result<Assistant, String> {
    let model_selection = get_butler_model_selection(app_handle).await?;
    let system_prompt = build_butler_system_prompt(app_handle).await?;
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;

    let existing = assistant_db
        .get_assistants()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(is_butler_system_assistant);

    let assistant_id = if let Some(assistant) = existing {
        assistant_db
            .update_assistant(
                assistant.id,
                BUTLER_SYSTEM_ASSISTANT_NAME,
                BUTLER_SYSTEM_ASSISTANT_DESCRIPTION,
            )
            .map_err(|e| e.to_string())?;
        assistant.id
    } else {
        assistant_db
            .add_assistant(
                BUTLER_SYSTEM_ASSISTANT_NAME,
                BUTLER_SYSTEM_ASSISTANT_DESCRIPTION,
                Some(0),
                true,
            )
            .map_err(|e| e.to_string())?
    };

    assistant_db
        .delete_assistant_prompt_param_by_assistant_id(assistant_id)
        .map_err(|e| e.to_string())?;
    assistant_db
        .delete_assistant_prompt_by_assistant_id(assistant_id)
        .map_err(|e| e.to_string())?;
    assistant_db
        .delete_assistant_model_config_by_assistant_id(assistant_id)
        .map_err(|e| e.to_string())?;
    assistant_db.delete_assistant_model_by_assistant_id(assistant_id).map_err(|e| e.to_string())?;

    assistant_db.add_assistant_prompt(assistant_id, &system_prompt).map_err(|e| e.to_string())?;
    let assistant_model_id = assistant_db
        .add_assistant_model(
            assistant_id,
            model_selection.provider_id,
            &model_selection.model_code,
            &model_selection.display_name,
        )
        .map_err(|e| e.to_string())?;

    for (name, value, value_type) in [
        ("max_tokens", "16000", "number"),
        ("temperature", "0.8", "float"),
        ("top_p", "1.0", "float"),
        ("stream", "true", "boolean"),
        ("use_native_toolcall", "true", "boolean"),
    ] {
        assistant_db
            .add_assistant_model_config(assistant_id, assistant_model_id, name, value, value_type)
            .map_err(|e| e.to_string())?;
    }

    ensure_agent_load_skill_for_assistant(app_handle, assistant_id)?;
    sync_butler_mcp_capabilities(app_handle, assistant_id)?;
    sync_butler_skill_capabilities(app_handle, assistant_id)?;

    assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())
}

pub(crate) async fn refresh_butler_system_assistant_if_ready(
    app_handle: &AppHandle,
) -> Result<(), String> {
    let butler_enabled = get_experimental_config_value(app_handle, "butler_experiment_enabled")
        .await
        .map(|value| parse_bool_flag(&value))
        .unwrap_or(false);
    if !butler_enabled {
        return Ok(());
    }

    let butler_model_configured = get_experimental_config_value(app_handle, "butler_model_id")
        .await
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    if !butler_model_configured {
        return Ok(());
    }

    let _ = ensure_butler_system_assistant(app_handle).await?;
    Ok(())
}

fn build_butler_conversation(
    assistant_id: i64,
    name: String,
    kind: &str,
    hidden: bool,
    parent_butler_conversation_id: Option<i64>,
    source_task_title: Option<String>,
    task_status: Option<String>,
) -> Conversation {
    let now = Utc::now();
    Conversation {
        id: 0,
        name,
        assistant_id: Some(assistant_id),
        created_time: now,
        updated_time: now,
        conversation_kind: kind.to_string(),
        parent_butler_conversation_id,
        source_task_title,
        is_hidden_from_normal_chat_list: hidden,
        channel_source: None,
        butler_task_status: task_status,
        butler_task_summary: None,
        butler_task_finalized_at: None,
    }
}

fn build_butler_archive_conversation_name(
    current_name: &str,
    archived_at: &DateTime<Utc>,
) -> String {
    let trimmed = current_name.trim();
    let base_name = if trimmed.is_empty() { "总管家主会话" } else { trimmed };
    format!("{}（存档 {}）", base_name, archived_at.format("%Y-%m-%d %H:%M:%S"))
}

fn summarize_text(text: &str) -> String {
    let compact =
        text.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
    let mut chars = compact.chars();
    let summary: String = chars.by_ref().take(160).collect();
    if chars.next().is_some() {
        format!("{}...", summary)
    } else if summary.is_empty() {
        "任务已完成".to_string()
    } else {
        summary
    }
}

fn normalize_summary(summary: Option<String>, fallback: &str) -> String {
    summary.filter(|value| !value.trim().is_empty()).unwrap_or_else(|| summarize_text(fallback))
}

fn trim_chars(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let trimmed: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", trimmed)
    } else {
        trimmed
    }
}

fn describe_task_terminal_status(status: &str) -> &'static str {
    match status {
        STATUS_SUCCEEDED => "已完成",
        STATUS_FAILED => "失败",
        STATUS_CANCELLED => "已取消",
        _ => "已结束",
    }
}

pub(crate) fn resolve_butler_execution_window(app_handle: &AppHandle) -> Result<Window, String> {
    for label in ["butler_experiment", "chat_ui", "ask"] {
        if let Some(window) = app_handle.get_webview_window(label) {
            return Ok(window.as_ref().window());
        }
    }
    Err("No available window for butler continuation".to_string())
}

fn resolve_or_create_butler_execution_window(app_handle: &AppHandle) -> Result<Window, String> {
    if let Ok(window) = resolve_butler_execution_window(app_handle) {
        return Ok(window);
    }

    crate::window::create_chat_ui_window_hidden(app_handle);
    app_handle
        .get_webview_window("chat_ui")
        .map(|window| window.as_ref().window())
        .ok_or_else(|| "No available window for butler continuation".to_string())
}

pub(crate) async fn wait_for_butler_main_to_be_idle(app_handle: &AppHandle, conversation_id: i64) {
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    for _ in 0..600 {
        if !activity_manager.get_runtime_state(conversation_id).await.is_running {
            return;
        }
        sleep(Duration::from_millis(250)).await;
    }
}

fn build_butler_task_result_system_message(
    task: &ButlerTaskListItem,
    definition: &ButlerTaskDefinition,
    result: &ButlerTaskResult,
    cancel_requested: bool,
) -> String {
    let detail_text = result
        .structured_output_json
        .as_deref()
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
        .and_then(|value| {
            value.get("content").and_then(|content| content.as_str()).map(str::to_string)
        })
        .unwrap_or_else(|| result.summary.clone().unwrap_or_default());
    let detail_text = if detail_text.trim().is_empty() {
        "无".to_string()
    } else {
        trim_chars(detail_text.trim(), TASK_RESULT_DETAIL_LIMIT)
    };
    let summary =
        result.summary.as_deref().filter(|value| !value.trim().is_empty()).unwrap_or("无");

    format!(
        "<butler_task_result>\nstatus={status}\ncancel_requested={cancel_requested}\ntask_conversation_id={task_conversation_id}\nfinal_message_id={final_message_id}\ntitle={title}\ngoal={goal}\nexecutor_assistant_id={assistant_id}\nexecutor_assistant_name={assistant_name}\nresult_handling_mode={result_handling_mode}\nnotification_policy={notification_policy}\nhandoff_contract_json={handoff_contract_json}\nsummary={summary}\ndetail_excerpt={detail_excerpt}\npayload_json={payload_json}\nstructured_output_json={structured_output_json}\n</butler_task_result>",
        status = task.status,
        cancel_requested = cancel_requested,
        task_conversation_id = task.task_conversation_id,
        final_message_id = result.final_message_id.map(|value| value.to_string()).unwrap_or_else(|| "null".to_string()),
        title = definition.title,
        goal = trim_chars(&definition.goal, 600),
        assistant_id = definition.executor_assistant_id,
        assistant_name = task.executor_assistant_name,
        result_handling_mode = definition.result_handling_mode.as_deref().unwrap_or("notify_only"),
        notification_policy = definition.notification_policy.as_deref().unwrap_or("default"),
        handoff_contract_json = definition.handoff_contract_json.as_deref().unwrap_or("null"),
        summary = summary,
        detail_excerpt = detail_text,
        payload_json = result.payload_json.as_deref().unwrap_or("null"),
        structured_output_json = result.structured_output_json.as_deref().unwrap_or("null"),
    )
}

fn build_butler_task_result_followup_prompt(
    task: &ButlerTaskListItem,
    cancel_requested: bool,
) -> String {
    format!(
        "系统任务回流：任务《{title}》{status}。这不是终端用户的新需求，而是刚完成/失败的子任务回调。请基于最新注入的 <butler_task_result> 信息立即判断下一步：必要时继续派工、修正方案、重试，或直接向用户汇报当前结论。{cancel_suffix}",
        title = task.title,
        status = describe_task_terminal_status(&task.status),
        cancel_suffix = if cancel_requested {
            " 这次回流伴随过取消请求，请特别判断结果是否仍可用。"
        } else {
            ""
        },
    )
}

async fn enqueue_butler_main_followup(
    app_handle: AppHandle,
    task: ButlerTaskListItem,
    definition: ButlerTaskDefinition,
    result: ButlerTaskResult,
    cancel_requested: bool,
    inject_system_message: bool,
) -> Result<(), String> {
    let continuation_lock =
        get_butler_main_continuation_lock(definition.butler_conversation_id).await;
    let _guard = continuation_lock.lock().await;
    wait_for_butler_main_to_be_idle(&app_handle, definition.butler_conversation_id).await;

    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let mut handoff_message_id = result.handoff_message_id;
    let followup_result: Result<(), String> = async {
        let Some(main_conversation) =
            conversation_repo.read(definition.butler_conversation_id).map_err(|e| e.to_string())?
        else {
            return Err("总管家主会话不存在".to_string());
        };
        if main_conversation.conversation_kind != BUTLER_KIND_MAIN {
            return Err("总管家主会话已归档，无法继续回流".to_string());
        }
        let assistant_id = main_conversation
            .assistant_id
            .ok_or_else(|| "总管家主会话缺少 assistant".to_string())?;

        if inject_system_message {
            let handoff_system_message = build_butler_task_result_system_message(
                &task,
                &definition,
                &result,
                cancel_requested,
            );
            let handoff_message = add_message(
                &app_handle,
                None,
                definition.butler_conversation_id,
                "system".to_string(),
                handoff_system_message,
                None,
                None,
                None,
                None,
                0,
                None,
                None,
            )
            .map_err(|e| e.to_string())?;
            handoff_message_id = Some(handoff_message.id);
            butler_repo
                .update_task_result_followup_state(
                    task.task_conversation_id,
                    FOLLOWUP_STATUS_HANDOFF_INJECTED,
                    handoff_message_id,
                )
                .map_err(|e| e.to_string())?;
        }

        let window = resolve_or_create_butler_execution_window(&app_handle)?;
        let ai_request = AiRequest {
            conversation_id: definition.butler_conversation_id.to_string(),
            assistant_id,
            prompt: build_butler_task_result_followup_prompt(&task, cancel_requested),
            model: None,
            override_model_id: None,
            temperature: None,
            top_p: None,
            max_tokens: None,
            stream: Some(true),
            attachment_list: None,
        };

        ask_ai(
            app_handle.clone(),
            app_handle.state::<crate::AppState>(),
            app_handle.state::<crate::AcpSessionState>(),
            app_handle.state::<crate::FeatureConfigState>(),
            app_handle.state::<crate::state::message_token::MessageTokenManager>(),
            app_handle.state::<crate::state::activity_state::ConversationActivityManager>(),
            window,
            ai_request,
            None,
            None,
            None,
            None,
            Some("internal".to_string()),
        )
        .await
        .map_err(|e| e.to_string())?;

        butler_repo
            .update_task_result_followup_state(
                task.task_conversation_id,
                FOLLOWUP_STATUS_ENQUEUED,
                handoff_message_id,
            )
            .map_err(|e| e.to_string())?;

        Ok(())
    }
    .await;

    if let Err(error) = followup_result {
        let rollback_status = if handoff_message_id.is_some() {
            FOLLOWUP_STATUS_HANDOFF_INJECTED
        } else {
            FOLLOWUP_STATUS_PENDING
        };
        if let Err(rollback_error) = butler_repo.update_task_result_followup_state(
            task.task_conversation_id,
            rollback_status,
            handoff_message_id,
        ) {
            warn!(
                task_conversation_id = task.task_conversation_id,
                error = %rollback_error,
                "failed to rollback butler task follow-up state"
            );
        }
        return Err(error);
    }

    Ok(())
}

fn spawn_butler_main_followup(
    app_handle: &AppHandle,
    task: &ButlerTaskListItem,
    definition: &ButlerTaskDefinition,
    result: &ButlerTaskResult,
    cancel_requested: bool,
    inject_system_message: bool,
) {
    let app_handle_clone = app_handle.clone();
    let task_clone = task.clone();
    let definition_clone = definition.clone();
    let result_clone = result.clone();
    let error_app_handle = app_handle_clone.clone();
    let error_butler_conversation_id = definition_clone.butler_conversation_id;
    let error_task_conversation_id = task_clone.task_conversation_id;
    let error_task_title = definition_clone.title.clone();

    std::thread::spawn(move || {
        let thread_result: Result<(), String> = tauri::async_runtime::block_on(async move {
            enqueue_butler_main_followup(
                app_handle_clone.clone(),
                task_clone,
                definition_clone,
                result_clone,
                cancel_requested,
                inject_system_message,
            )
            .await
        });

        if let Err(error) = thread_result {
            warn!(
                butler_conversation_id = error_butler_conversation_id,
                task_conversation_id = error_task_conversation_id,
                error = %error,
                "failed to enqueue butler main follow-up"
            );
            emit_butler_notification(
                &error_app_handle,
                error_butler_conversation_id,
                error_task_conversation_id,
                "task_followup_failed",
                format!("任务 {} 已回流，但总管家续跑失败", error_task_title),
                error,
                "medium",
            );
        }
    });
}

fn schedule_butler_main_followup(
    app_handle: &AppHandle,
    task: &ButlerTaskListItem,
    definition: &ButlerTaskDefinition,
    result: &ButlerTaskResult,
    cancel_requested: bool,
) {
    let claim_result = (|| -> Result<bool, String> {
        let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        db.butler_repo()
            .map_err(|e| e.to_string())?
            .try_mark_task_result_followup_dispatching(
                task.task_conversation_id,
                result.handoff_message_id,
            )
            .map_err(|e| e.to_string())
    })();

    match claim_result {
        Ok(true) => {
            spawn_butler_main_followup(
                app_handle,
                task,
                definition,
                result,
                cancel_requested,
                true,
            );
        }
        Ok(false) => {
            debug!(
                task_conversation_id = task.task_conversation_id,
                "skip scheduling butler main follow-up because another dispatcher already claimed it"
            );
        }
        Err(error) => {
            warn!(
                task_conversation_id = task.task_conversation_id,
                error = %error,
                "failed to claim butler main follow-up before scheduling"
            );
        }
    }
}

fn dedupe_messages(
    raw_messages: Vec<(Message, Option<crate::db::conversation_db::MessageAttachment>)>,
) -> Vec<Message> {
    let mut seen = HashSet::new();
    let mut messages = Vec::new();
    for (message, _) in raw_messages {
        if seen.insert(message.id) {
            messages.push(message);
        }
    }
    messages.sort_by(|left, right| {
        left.created_time.cmp(&right.created_time).then(left.id.cmp(&right.id))
    });
    messages
}

fn emit_butler_notification(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
    task_conversation_id: i64,
    notification_type: &str,
    title: String,
    body: String,
    importance: &str,
) {
    let payload = ButlerNotificationEvent {
        butler_conversation_id,
        task_conversation_id,
        notification_type: notification_type.to_string(),
        title,
        body,
        importance: importance.to_string(),
    };
    let _ = app_handle.emit("butler_notification_created", payload);
}

fn format_permission_kind_label(permission_kind: &str) -> &'static str {
    match permission_kind {
        "acp" => "ACP 工具权限",
        _ => "操作权限",
    }
}

pub(crate) async fn emit_butler_task_permission_state_changed(
    app_handle: &AppHandle,
    task_conversation_id: i64,
    permission_kind: &str,
    requested: bool,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let Some(conversation) =
        conversation_repo.read(task_conversation_id).map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Ok(());
    }
    let Some(definition) = butler_repo
        .get_task_definition_by_task_conversation_id(task_conversation_id)
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let result = butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?;
    let task_item =
        build_task_list_item(app_handle, conversation, &definition, result.as_ref()).await?;
    let _ = app_handle.emit("butler_task_updated", task_item.clone());

    let permission_label = format_permission_kind_label(permission_kind);
    if requested {
        emit_butler_notification(
            app_handle,
            definition.butler_conversation_id,
            task_conversation_id,
            "permission_requested",
            format!("任务 {} 等待{}审批", definition.title, permission_label),
            "请在总管家窗口中确认后继续执行。".to_string(),
            "medium",
        );
        spawn_butler_task_attention_followup(app_handle, &task_item, &definition, permission_kind);
    } else {
        emit_butler_notification(
            app_handle,
            definition.butler_conversation_id,
            task_conversation_id,
            "permission_resolved",
            format!("任务 {} 的{}请求已处理", definition.title, permission_label),
            if task_item.has_pending_permission {
                "该任务仍有其他待处理权限请求。".to_string()
            } else {
                "任务执行将继续推进。".to_string()
            },
            "light",
        );
    }

    Ok(())
}

/// Notify Butler main conversation that a sub-task is waiting on ask_user_question.
pub(crate) async fn emit_butler_task_ask_user_attention(
    app_handle: &AppHandle,
    task_conversation_id: i64,
) -> Result<(), String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let Some(conversation) =
        conversation_repo.read(task_conversation_id).map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Ok(());
    }
    let Some(definition) = butler_repo
        .get_task_definition_by_task_conversation_id(task_conversation_id)
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    let result = butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?;
    let task_item =
        build_task_list_item(app_handle, conversation, &definition, result.as_ref()).await?;
    let _ = app_handle.emit("butler_task_updated", task_item.clone());

    emit_butler_notification(
        app_handle,
        definition.butler_conversation_id,
        task_conversation_id,
        "ask_user_question",
        format!("任务 {} 需要你回答问题", definition.title),
        "子任务助手向你提出了一个问题，请使用 task_conversation_operation read 查看并回答。"
            .to_string(),
        "medium",
    );
    spawn_butler_task_attention_followup(app_handle, &task_item, &definition, "ask_user_question");

    Ok(())
}

async fn build_task_list_item(
    app_handle: &AppHandle,
    conversation: Conversation,
    definition: &ButlerTaskDefinition,
    result: Option<&ButlerTaskResult>,
) -> Result<ButlerTaskListItem, String> {
    let assistant_name = {
        let name_cache = app_handle.state::<crate::NameCacheState>();
        let cache = name_cache.assistant_names.lock().await;
        cache.get(&definition.executor_assistant_id).cloned().unwrap_or_else(|| "未知".to_string())
    };
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let runtime_state = activity_manager.get_runtime_state(conversation.id).await;
    let status = conversation.butler_task_status.clone().unwrap_or_else(|| {
        if runtime_state.is_running { STATUS_RUNNING } else { STATUS_ACCEPTED }.to_string()
    });
    let last_summary = result
        .and_then(|value| value.summary.clone())
        .or_else(|| conversation.butler_task_summary.clone());
    let operation_state = app_handle.state::<OperationState>();
    let acp_permission_state = app_handle.state::<AcpPermissionState>();
    let has_pending_permission =
        operation_state.has_pending_permission_for_conversation(conversation.id).await
            || acp_permission_state.has_pending_permission_for_conversation(conversation.id).await;

    Ok(ButlerTaskListItem {
        butler_conversation_id: definition.butler_conversation_id,
        task_conversation_id: conversation.id,
        title: definition.title.clone(),
        goal: definition.goal.clone(),
        status,
        executor_assistant_id: definition.executor_assistant_id,
        executor_assistant_name: assistant_name,
        last_summary,
        created_time: conversation.created_time,
        updated_time: conversation.updated_time,
        finalized_at: conversation.butler_task_finalized_at,
        is_finalized: conversation.butler_task_finalized_at.is_some(),
        has_pending_permission,
        is_running: runtime_state.is_running,
    })
}

async fn list_butler_tasks_internal(
    app_handle: &AppHandle,
    butler_conversation_id: i64,
) -> Result<Vec<ButlerTaskListItem>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let definitions =
        butler_repo.list_task_definitions(butler_conversation_id).map_err(|e| e.to_string())?;
    let definition_map: HashMap<i64, ButlerTaskDefinition> = definitions
        .into_iter()
        .map(|definition| (definition.task_conversation_id, definition))
        .collect();

    let task_conversations = conversation_repo
        .list_by_parent_butler_conversation_id(butler_conversation_id)
        .map_err(|e| e.to_string())?;
    let mut tasks = Vec::new();
    for conversation in task_conversations
        .into_iter()
        .filter(|conversation| conversation.conversation_kind == BUTLER_KIND_TASK)
    {
        if let Some(definition) = definition_map.get(&conversation.id) {
            let result = butler_repo.get_task_result(conversation.id).map_err(|e| e.to_string())?;
            tasks.push(
                build_task_list_item(app_handle, conversation, definition, result.as_ref()).await?,
            );
        }
    }
    Ok(tasks)
}

pub(crate) async fn load_or_create_butler_main_internal(
    app_handle: &AppHandle,
) -> Result<Conversation, String> {
    ensure_butler_enabled(app_handle).await?;
    let butler_assistant = ensure_butler_system_assistant(app_handle).await?;
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;

    let conversation = if let Some(state) =
        butler_repo.get_main_state(BUTLER_MAIN_SLOT).map_err(|e| e.to_string())?
    {
        match conversation_repo.read(state.butler_conversation_id).map_err(|e| e.to_string())? {
            Some(mut conversation) => {
                let mut needs_update = false;
                if conversation.conversation_kind != BUTLER_KIND_MAIN {
                    conversation.conversation_kind = BUTLER_KIND_MAIN.to_string();
                    needs_update = true;
                }
                if !conversation.is_hidden_from_normal_chat_list {
                    conversation.is_hidden_from_normal_chat_list = true;
                    needs_update = true;
                }
                if conversation.assistant_id != Some(butler_assistant.id) {
                    conversation.assistant_id = Some(butler_assistant.id);
                    needs_update = true;
                }
                if needs_update {
                    conversation.updated_time = Utc::now();
                    conversation_repo.update(&conversation).map_err(|e| e.to_string())?;
                }
                cleanup_butler_external_channel_system_messages(app_handle, conversation.id)?;
                conversation
            }
            None => {
                let created = conversation_repo
                    .create(&build_butler_conversation(
                        butler_assistant.id,
                        "总管家主会话".to_string(),
                        BUTLER_KIND_MAIN,
                        true,
                        None,
                        None,
                        None,
                    ))
                    .map_err(|e| e.to_string())?;
                butler_repo
                    .upsert_main_state(created.id, BUTLER_MAIN_SLOT)
                    .map_err(|e| e.to_string())?;
                cleanup_butler_external_channel_system_messages(app_handle, created.id)?;
                created
            }
        }
    } else {
        let created = conversation_repo
            .create(&build_butler_conversation(
                butler_assistant.id,
                "总管家主会话".to_string(),
                BUTLER_KIND_MAIN,
                true,
                None,
                None,
                None,
            ))
            .map_err(|e| e.to_string())?;
        butler_repo.upsert_main_state(created.id, BUTLER_MAIN_SLOT).map_err(|e| e.to_string())?;
        cleanup_butler_external_channel_system_messages(app_handle, created.id)?;
        created
    };

    butler_repo.touch_main_state(BUTLER_MAIN_SLOT).map_err(|e| e.to_string())?;
    let _ = app_handle.emit("butler_main_loaded", json!({ "conversation_id": conversation.id }));
    Ok(conversation)
}

fn resolve_executor_assistant(
    app_handle: &AppHandle,
    request: &SpawnButlerTaskRequest,
) -> Result<(Assistant, String), String> {
    let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    if let Some(assistant_id) = request.executor_assistant_id {
        let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
        if is_butler_system_assistant(&assistant) {
            return Err("系统保留的总管家助手不能作为执行助手".to_string());
        }
        return Ok((assistant, "assistant_id".to_string()));
    }

    if let Some(name) = request
        .executor_assistant_name
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
    {
        let assistants =
            visible_executor_candidates(assistant_db.get_assistants().map_err(|e| e.to_string())?);
        let assistant = assistants
            .into_iter()
            .find(|assistant| assistant.name.eq_ignore_ascii_case(name) || assistant.name == name)
            .ok_or_else(|| format!("未找到名为 '{}' 的执行助手", name))?;
        return Ok((assistant, "assistant_name".to_string()));
    }

    Ok((resolve_default_executor_assistant(&assistant_db)?, "default".to_string()))
}

async fn record_terminal_task_state(
    app_handle: &AppHandle,
    task_conversation_id: i64,
    status: &str,
    summary: String,
    detail_text: String,
    final_message_id: Option<i64>,
    cancel_requested: bool,
) -> Result<ButlerTaskListItem, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let conversation = conversation_repo
        .read(task_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务会话不存在".to_string())?;
    let definition = butler_repo
        .get_task_definition_by_task_conversation_id(task_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务定义不存在".to_string())?;
    let existing_result =
        butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?;
    let now = Utc::now();
    let result = ButlerTaskResult {
        id: existing_result.as_ref().map(|value| value.id).unwrap_or(0),
        task_conversation_id,
        handoff_mode: definition.result_handling_mode.clone(),
        payload_json: Some(
            json!({
                "status": status,
                "task_conversation_id": task_conversation_id,
                "final_message_id": final_message_id,
                "cancel_requested": cancel_requested,
            })
            .to_string(),
        ),
        summary: Some(summary.clone()),
        structured_output_json: Some(
            json!({
                "status": status,
                "content": detail_text,
                "cancel_requested": cancel_requested,
            })
            .to_string(),
        ),
        evidence_json: Some("[]".to_string()),
        artifact_refs_json: Some("[]".to_string()),
        followup_suggestions_json: Some("[]".to_string()),
        followup_status: Some(FOLLOWUP_STATUS_PENDING.to_string()),
        handoff_message_id: None,
        final_message_id,
        created_time: existing_result.as_ref().map(|value| value.created_time).unwrap_or(now),
        updated_time: now,
    };
    let saved_result = butler_repo.upsert_task_result(&result).map_err(|e| e.to_string())?;
    butler_repo
        .update_task_conversation_state(
            task_conversation_id,
            status,
            Some(summary.as_str()),
            Some(now),
        )
        .map_err(|e| e.to_string())?;
    let updated_conversation = conversation_repo
        .read(task_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务会话不存在".to_string())?;
    let task_item =
        build_task_list_item(app_handle, updated_conversation, &definition, Some(&saved_result))
            .await?;

    let _ = app_handle.emit("butler_task_updated", task_item.clone());
    let _ = app_handle.emit("butler_task_finalized", task_item.clone());
    let _ = app_handle.emit(
        "butler_task_result_available",
        ButlerTaskResultAvailableEvent { task: task_item.clone(), result: saved_result.clone() },
    );
    emit_butler_notification(
        app_handle,
        definition.butler_conversation_id,
        task_conversation_id,
        "task_terminal",
        format!(
            "任务 {} 已{}{}",
            definition.title,
            if status == STATUS_SUCCEEDED {
                "完成"
            } else if status == STATUS_CANCELLED {
                "取消"
            } else {
                "失败"
            },
            if cancel_requested && status == STATUS_SUCCEEDED {
                "（取消请求后仍返回结果）"
            } else {
                ""
            },
        ),
        summary.clone(),
        if status == STATUS_FAILED { "medium" } else { "light" },
    );
    schedule_butler_main_followup(
        app_handle,
        &task_item,
        &definition,
        &saved_result,
        cancel_requested,
    );

    debug!(
        task_conversation_id,
        butler_conversation_id = conversation.parent_butler_conversation_id,
        status,
        "recorded terminal butler task state"
    );
    Ok(task_item)
}

fn find_latest_task_message<'a>(
    messages: &'a [Message],
    message_type: &str,
    require_finish_time: bool,
) -> Option<&'a Message> {
    messages.iter().rev().find(|message| {
        message.message_type == message_type
            && !message.content.trim().is_empty()
            && (!require_finish_time || message.finish_time.is_some())
    })
}

fn find_latest_terminal_response_message(
    messages: &[Message],
    require_finish_time: bool,
) -> Option<&Message> {
    messages.iter().rev().find(|message| {
        message.message_type == "response"
            && !message.content.trim().is_empty()
            && !message.content.contains("<!-- MCP_TOOL_CALL:")
            && (!require_finish_time || message.finish_time.is_some())
    })
}

fn try_decide_butler_task_terminal_state(
    messages: &[Message],
    conversation_summary: Option<String>,
    cancel_requested: bool,
) -> Option<ButlerTaskTerminalDecision> {
    let latest_error = find_latest_task_message(messages, "error", false);

    if cancel_requested {
        if let Some(response) = find_latest_terminal_response_message(messages, true) {
            return Some(ButlerTaskTerminalDecision {
                status: STATUS_SUCCEEDED,
                summary: normalize_summary(
                    Some(format!(
                        "任务在取消请求后仍返回最终结果：{}",
                        summarize_text(&response.content)
                    )),
                    &response.content,
                ),
                detail_text: response.content.clone(),
                final_message_id: Some(response.id),
                cancel_requested: true,
            });
        }

        let cancel_detail = latest_error
            .map(|error| error.content.clone())
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "任务已取消，未产出最终结果".to_string());
        return Some(ButlerTaskTerminalDecision {
            status: STATUS_CANCELLED,
            summary: normalize_summary(
                Some(format!("任务已取消：{}", summarize_text(&cancel_detail))),
                &cancel_detail,
            ),
            detail_text: cancel_detail,
            final_message_id: latest_error.map(|error| error.id),
            cancel_requested: true,
        });
    }

    let latest_response = find_latest_terminal_response_message(messages, false);

    match (latest_response, latest_error) {
        (Some(response), Some(error)) if error.id > response.id => {
            Some(ButlerTaskTerminalDecision {
                status: STATUS_FAILED,
                summary: normalize_summary(Some(summarize_text(&error.content)), &error.content),
                detail_text: error.content.clone(),
                final_message_id: Some(error.id),
                cancel_requested: false,
            })
        }
        (Some(response), _) => Some(ButlerTaskTerminalDecision {
            status: STATUS_SUCCEEDED,
            summary: normalize_summary(Some(summarize_text(&response.content)), &response.content),
            detail_text: response.content.clone(),
            final_message_id: Some(response.id),
            cancel_requested: false,
        }),
        (_, Some(error)) => Some(ButlerTaskTerminalDecision {
            status: STATUS_FAILED,
            summary: normalize_summary(Some(summarize_text(&error.content)), &error.content),
            detail_text: error.content.clone(),
            final_message_id: Some(error.id),
            cancel_requested: false,
        }),
        _ => conversation_summary.filter(|summary| !summary.trim().is_empty()).map(|summary| {
            ButlerTaskTerminalDecision {
                status: STATUS_FAILED,
                summary: normalize_summary(Some(summary), "任务结束，但未生成可用结果"),
                detail_text: "任务结束，但未生成可用结果".to_string(),
                final_message_id: None,
                cancel_requested: false,
            }
        }),
    }
}

pub(crate) async fn finalize_butler_task_if_ready(
    app_handle: &AppHandle,
    task_conversation_id: i64,
) -> Result<Option<ButlerTaskListItem>, String> {
    let finalization_lock = get_butler_task_finalization_lock(task_conversation_id).await;
    let _guard = finalization_lock.lock().await;

    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let Some(conversation) =
        conversation_repo.read(task_conversation_id).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Ok(None);
    }
    if conversation.butler_task_finalized_at.is_some()
        && butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?.is_some()
    {
        let definition = butler_repo
            .get_task_definition_by_task_conversation_id(task_conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "任务定义不存在".to_string())?;
        let result =
            butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?;
        return Ok(Some(
            build_task_list_item(app_handle, conversation, &definition, result.as_ref()).await?,
        ));
    }

    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    if activity_manager.get_runtime_state(task_conversation_id).await.is_running {
        return Ok(None);
    }

    let messages = dedupe_messages(
        db.message_repo()
            .map_err(|e| e.to_string())?
            .list_by_conversation_id(task_conversation_id)
            .map_err(|e| e.to_string())?,
    );
    let Some(decision) = try_decide_butler_task_terminal_state(
        &messages,
        conversation.butler_task_summary.clone(),
        conversation.butler_task_status.as_deref() == Some(STATUS_CANCELLED),
    ) else {
        return Ok(None);
    };

    record_terminal_task_state(
        app_handle,
        task_conversation_id,
        decision.status,
        decision.summary,
        decision.detail_text,
        decision.final_message_id,
        decision.cancel_requested,
    )
    .await
    .map(Some)
}

fn update_butler_task_watcher_state(
    seen_running: &mut bool,
    idle_checks: &mut usize,
    is_running: bool,
) -> bool {
    if is_running {
        *seen_running = true;
        *idle_checks = 0;
        return false;
    }

    if *seen_running {
        *idle_checks += 1;
        return *idle_checks >= BUTLER_TASK_WATCHER_IDLE_OBSERVATIONS;
    }

    false
}

fn parse_task_conversation_id_from_handoff_message(content: &str) -> Option<i64> {
    if !content.contains("<butler_task_result>") {
        return None;
    }
    content
        .lines()
        .find_map(|line| line.strip_prefix("task_conversation_id=")?.trim().parse::<i64>().ok())
}

fn find_latest_handoff_message_id(messages: &[Message], task_conversation_id: i64) -> Option<i64> {
    messages.iter().rev().find_map(|message| {
        (message.message_type == "system"
            && parse_task_conversation_id_from_handoff_message(&message.content)
                == Some(task_conversation_id))
        .then_some(message.id)
    })
}

fn parse_task_conversation_id_from_attention_message(content: &str) -> Option<i64> {
    if !content.contains("<butler_task_attention>") {
        return None;
    }
    content
        .lines()
        .find_map(|line| line.strip_prefix("task_conversation_id=")?.trim().parse::<i64>().ok())
}

fn find_latest_attention_message_id(
    messages: &[Message],
    task_conversation_id: i64,
) -> Option<i64> {
    messages.iter().rev().find_map(|message| {
        (message.message_type == "system"
            && parse_task_conversation_id_from_attention_message(&message.content)
                == Some(task_conversation_id))
        .then_some(message.id)
    })
}

fn has_non_system_message_after(messages: &[Message], message_id: i64) -> bool {
    messages.iter().any(|message| message.id > message_id && message.message_type != "system")
}

fn build_butler_task_attention_system_message(
    task: &ButlerTaskListItem,
    definition: &ButlerTaskDefinition,
    attention_kind: &str,
    latest_message_excerpt: &str,
    operation_permission_count: usize,
    acp_permission_count: usize,
    ask_user_question_count: usize,
) -> String {
    format!(
        "<butler_task_attention>\nattention_kind={attention_kind}\ntask_conversation_id={task_conversation_id}\ntitle={title}\ngoal={goal}\nstatus={status}\nexecutor_assistant_id={assistant_id}\nexecutor_assistant_name={assistant_name}\noperation_permission_count={operation_permission_count}\nacp_permission_count={acp_permission_count}\nask_user_question_count={ask_user_question_count}\nlatest_message_excerpt={latest_message_excerpt}\n</butler_task_attention>",
        attention_kind = attention_kind,
        task_conversation_id = task.task_conversation_id,
        title = definition.title,
        goal = trim_chars(&definition.goal, 600),
        status = task.status,
        assistant_id = definition.executor_assistant_id,
        assistant_name = task.executor_assistant_name,
        operation_permission_count = operation_permission_count,
        acp_permission_count = acp_permission_count,
        ask_user_question_count = ask_user_question_count,
        latest_message_excerpt = latest_message_excerpt,
    )
}

fn build_butler_task_attention_followup_prompt(
    task: &ButlerTaskListItem,
    attention_kind: &str,
) -> String {
    format!(
        "系统任务提醒：任务《{title}》出现 {attention_kind}。这不是终端用户的新需求，而是子任务运行中的内部阻塞提醒。请优先使用 `task_conversation_operation` 查看该 task conversation 的最新消息与待处理权限，必要时直接确认权限或补充新的提示，然后再决定是否继续等待或向用户同步。",
        title = task.title,
        attention_kind = attention_kind,
    )
}

async fn enqueue_butler_task_attention_followup(
    app_handle: AppHandle,
    task: ButlerTaskListItem,
    definition: ButlerTaskDefinition,
    attention_kind: String,
) -> Result<(), String> {
    let continuation_lock =
        get_butler_main_continuation_lock(definition.butler_conversation_id).await;
    let _guard = continuation_lock.lock().await;
    wait_for_butler_main_to_be_idle(&app_handle, definition.butler_conversation_id).await;

    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let message_repo = db.message_repo().map_err(|e| e.to_string())?;
    let Some(main_conversation) =
        conversation_repo.read(definition.butler_conversation_id).map_err(|e| e.to_string())?
    else {
        return Err("总管家主会话不存在".to_string());
    };
    if main_conversation.conversation_kind != BUTLER_KIND_MAIN {
        return Err("总管家主会话已归档，无法继续处理子任务提醒".to_string());
    }
    let assistant_id =
        main_conversation.assistant_id.ok_or_else(|| "总管家主会话缺少 assistant".to_string())?;

    let main_messages = dedupe_messages(
        message_repo
            .list_by_conversation_id(definition.butler_conversation_id)
            .map_err(|e| e.to_string())?,
    );
    if let Some(existing_attention_message_id) =
        find_latest_attention_message_id(&main_messages, task.task_conversation_id)
    {
        if !has_non_system_message_after(&main_messages, existing_attention_message_id) {
            return Ok(());
        }
    }

    let task_messages = dedupe_messages(
        message_repo
            .list_by_conversation_id(task.task_conversation_id)
            .map_err(|e| e.to_string())?,
    );
    let latest_message_excerpt = task_messages
        .iter()
        .rev()
        .find(|message| message.message_type != "system")
        .map(|message| trim_chars(message.content.trim(), 500))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "无".to_string());
    let operation_permission_count = app_handle
        .state::<OperationState>()
        .list_permission_requests_for_conversation(task.task_conversation_id)
        .await
        .len();
    let acp_permission_count = app_handle
        .state::<AcpPermissionState>()
        .list_requests_for_conversation(task.task_conversation_id)
        .await
        .len();
    let ask_user_question_count = app_handle
        .state::<crate::mcp::builtin_mcp::interaction::InteractionState>()
        .list_requests_for_conversation(task.task_conversation_id)
        .await
        .len();

    add_message(
        &app_handle,
        None,
        definition.butler_conversation_id,
        "system".to_string(),
        build_butler_task_attention_system_message(
            &task,
            &definition,
            &attention_kind,
            &latest_message_excerpt,
            operation_permission_count,
            acp_permission_count,
            ask_user_question_count,
        ),
        None,
        None,
        None,
        None,
        0,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    let window = resolve_or_create_butler_execution_window(&app_handle)?;
    let ai_request = AiRequest {
        conversation_id: definition.butler_conversation_id.to_string(),
        assistant_id,
        prompt: build_butler_task_attention_followup_prompt(&task, &attention_kind),
        model: None,
        override_model_id: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: Some(true),
        attachment_list: None,
    };

    ask_ai(
        app_handle.clone(),
        app_handle.state::<crate::AppState>(),
        app_handle.state::<crate::AcpSessionState>(),
        app_handle.state::<crate::FeatureConfigState>(),
        app_handle.state::<crate::state::message_token::MessageTokenManager>(),
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>(),
        window,
        ai_request,
        None,
        None,
        None,
        None,
        Some("internal".to_string()),
    )
    .await
    .map_err(|e| e.to_string())?;

    Ok(())
}

fn spawn_butler_task_attention_followup(
    app_handle: &AppHandle,
    task: &ButlerTaskListItem,
    definition: &ButlerTaskDefinition,
    attention_kind: &str,
) {
    let app_handle_clone = app_handle.clone();
    let task_clone = task.clone();
    let definition_clone = definition.clone();
    let attention_kind = attention_kind.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(error) = enqueue_butler_task_attention_followup(
            app_handle_clone,
            task_clone.clone(),
            definition_clone,
            attention_kind.clone(),
        )
        .await
        {
            warn!(
                task_conversation_id = task_clone.task_conversation_id,
                attention_kind = %attention_kind,
                error = %error,
                "failed to enqueue butler task attention followup"
            );
        }
    });
}

async fn ensure_butler_task_followup_if_needed(
    app_handle: &AppHandle,
    task_conversation_id: i64,
) -> Result<bool, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let Some(conversation) =
        conversation_repo.read(task_conversation_id).map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Ok(false);
    }
    let Some(definition) = butler_repo
        .get_task_definition_by_task_conversation_id(task_conversation_id)
        .map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let Some(mut result) =
        butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?
    else {
        return Ok(false);
    };
    let followup_status = result.followup_status.as_deref().unwrap_or(FOLLOWUP_STATUS_ENQUEUED);
    if followup_status == FOLLOWUP_STATUS_ENQUEUED || followup_status == FOLLOWUP_STATUS_DISPATCHING
    {
        return Ok(false);
    }

    let main_messages = dedupe_messages(
        db.message_repo()
            .map_err(|e| e.to_string())?
            .list_by_conversation_id(definition.butler_conversation_id)
            .map_err(|e| e.to_string())?,
    );
    let latest_handoff_message_id = result
        .handoff_message_id
        .or_else(|| find_latest_handoff_message_id(&main_messages, task_conversation_id));
    result.handoff_message_id = latest_handoff_message_id;
    let inject_system_message = latest_handoff_message_id.is_none();
    if let Some(handoff_message_id) = latest_handoff_message_id {
        if has_non_system_message_after(&main_messages, handoff_message_id) {
            butler_repo
                .update_task_result_followup_state(
                    task_conversation_id,
                    FOLLOWUP_STATUS_ENQUEUED,
                    Some(handoff_message_id),
                )
                .map_err(|e| e.to_string())?;
            return Ok(false);
        }
    }

    if !butler_repo
        .try_mark_task_result_followup_dispatching(task_conversation_id, latest_handoff_message_id)
        .map_err(|e| e.to_string())?
    {
        return Ok(false);
    }

    let task_item =
        build_task_list_item(app_handle, conversation, &definition, Some(&result)).await?;
    spawn_butler_main_followup(
        app_handle,
        &task_item,
        &definition,
        &result,
        task_item.status == STATUS_CANCELLED,
        inject_system_message,
    );
    Ok(true)
}

pub(crate) fn spawn_butler_task_watcher(app_handle: AppHandle, task_conversation_id: i64) {
    tauri::async_runtime::spawn(async move {
        let activity_manager =
            app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
        let mut seen_running = false;
        let mut idle_checks = 0usize;

        loop {
            let runtime_state = activity_manager.get_runtime_state(task_conversation_id).await;
            if update_butler_task_watcher_state(
                &mut seen_running,
                &mut idle_checks,
                runtime_state.is_running,
            ) {
                match finalize_butler_task_if_ready(&app_handle, task_conversation_id).await {
                    Ok(Some(_)) => return,
                    Ok(None) => {
                        idle_checks = 0;
                    }
                    Err(error) => {
                        warn!(task_conversation_id, error = %error, "failed to finalize butler task");
                        idle_checks = 0;
                    }
                }
            }
            sleep(BUTLER_TASK_WATCHER_POLL_INTERVAL).await;
        }
    });
}

async fn reconcile_butler_tasks_once(app_handle: &AppHandle) -> Result<usize, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let task_ids = conversation_repo
        .list_reconcilable_butler_task_conversation_ids()
        .map_err(|e| e.to_string())?;

    let mut finalized_count = 0usize;
    for task_conversation_id in task_ids {
        match finalize_butler_task_if_ready(app_handle, task_conversation_id).await {
            Ok(Some(_)) => finalized_count += 1,
            Ok(None) => {}
            Err(error) => {
                warn!(
                    task_conversation_id,
                    error = %error,
                    "failed to reconcile butler task"
                );
            }
        }
    }

    let followup_task_ids = conversation_repo
        .list_butler_task_conversation_ids_pending_followup()
        .map_err(|e| e.to_string())?;
    for task_conversation_id in followup_task_ids {
        if let Err(error) =
            ensure_butler_task_followup_if_needed(app_handle, task_conversation_id).await
        {
            warn!(
                task_conversation_id,
                error = %error,
                "failed to reconcile butler task follow-up"
            );
        }
    }

    Ok(finalized_count)
}

pub(crate) fn spawn_butler_task_reconciler(app_handle: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            match reconcile_butler_tasks_once(&app_handle).await {
                Ok(finalized_count) if finalized_count > 0 => {
                    debug!(finalized_count, "reconciled butler tasks");
                }
                Ok(_) => {}
                Err(error) => {
                    warn!(error = %error, "failed to run butler task reconciliation");
                }
            }
            sleep(BUTLER_TASK_RECONCILE_INTERVAL).await;
        }
    });
}

pub(crate) async fn mark_butler_task_cancelled(
    app_handle: &AppHandle,
    task_conversation_id: i64,
) -> Result<Option<ButlerTaskListItem>, String> {
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let Some(conversation) =
        conversation_repo.read(task_conversation_id).map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Ok(None);
    }
    butler_repo
        .update_task_conversation_state(
            task_conversation_id,
            STATUS_CANCELLED,
            Some("任务已取消"),
            Some(Utc::now()),
        )
        .map_err(|e| e.to_string())?;
    finalize_butler_task_if_ready(app_handle, task_conversation_id).await
}

pub(crate) async fn spawn_butler_task_with_window(
    app_handle: &AppHandle,
    window: &Window,
    request: SpawnButlerTaskRequest,
) -> Result<SpawnButlerTaskResponse, String> {
    ensure_butler_enabled(app_handle).await?;

    let _ = ensure_butler_system_assistant(app_handle).await?;
    let db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;

    let butler_conversation = conversation_repo
        .read(request.butler_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "总管家主会话不存在".to_string())?;
    if butler_conversation.conversation_kind != BUTLER_KIND_MAIN {
        return Err("只能在总管家主会话下派发任务".to_string());
    }

    let (executor_assistant, executor_source) = resolve_executor_assistant(app_handle, &request)?;
    let title = request.title.trim();
    let goal = request.goal.trim();
    if title.is_empty() {
        return Err("任务标题不能为空".to_string());
    }
    if goal.is_empty() {
        return Err("任务目标不能为空".to_string());
    }

    let created_task = conversation_repo
        .create(&build_butler_conversation(
            executor_assistant.id,
            title.to_string(),
            BUTLER_KIND_TASK,
            true,
            Some(request.butler_conversation_id),
            Some(title.to_string()),
            Some(STATUS_ACCEPTED.to_string()),
        ))
        .map_err(|e| e.to_string())?;

    let definition = butler_repo
        .create_task_definition(&ButlerTaskDefinition {
            id: 0,
            butler_conversation_id: request.butler_conversation_id,
            task_conversation_id: created_task.id,
            title: title.to_string(),
            goal: goal.to_string(),
            executor_assistant_id: executor_assistant.id,
            executor_assistant_source: executor_source.clone(),
            permission_template_source: Some(executor_source.clone()),
            handoff_contract_json: request.handoff_contract_json.clone(),
            result_handling_mode: request
                .result_handling_mode
                .clone()
                .or_else(|| Some("notify_only".to_string())),
            notification_policy: request
                .notification_policy
                .clone()
                .or_else(|| Some("default".to_string())),
            created_time: Utc::now(),
        })
        .map_err(|e| e.to_string())?;

    let accepted_task =
        build_task_list_item(app_handle, created_task.clone(), &definition, None).await?;
    let _ = app_handle.emit("butler_task_created", accepted_task.clone());
    emit_butler_notification(
        app_handle,
        request.butler_conversation_id,
        created_task.id,
        "task_created",
        format!("任务 {} 已受理", definition.title),
        definition.goal.clone(),
        "light",
    );

    butler_repo
        .update_task_conversation_state(created_task.id, STATUS_RUNNING, None, None)
        .map_err(|e| e.to_string())?;

    let ai_request = AiRequest {
        conversation_id: created_task.id.to_string(),
        assistant_id: executor_assistant.id,
        prompt: definition.goal.clone(),
        model: None,
        override_model_id: None,
        temperature: None,
        top_p: None,
        max_tokens: None,
        stream: Some(true),
        attachment_list: None,
    };

    let updated_conversation = conversation_repo
        .read(created_task.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务会话不存在".to_string())?;
    let running_task =
        build_task_list_item(app_handle, updated_conversation, &definition, None).await?;
    let _ = app_handle.emit("butler_task_updated", running_task);

    let app_handle_clone = app_handle.clone();
    let window_clone = window.clone();
    let task_id = created_task.id;
    let butler_conversation_id = request.butler_conversation_id;
    let definition_clone = definition.clone();
    let executor_assistant_id = executor_assistant.id;
    let executor_assistant_name = executor_assistant.name.clone();
    std::thread::spawn(move || {
        let thread_result: Result<(), String> = tauri::async_runtime::block_on(async move {
            match ask_ai(
                app_handle_clone.clone(),
                app_handle_clone.state::<crate::AppState>(),
                app_handle_clone.state::<crate::AcpSessionState>(),
                app_handle_clone.state::<crate::FeatureConfigState>(),
                app_handle_clone.state::<crate::state::message_token::MessageTokenManager>(),
                app_handle_clone
                    .state::<crate::state::activity_state::ConversationActivityManager>(),
                window_clone,
                ai_request,
                None,
                None,
                None,
                None,
                Some("internal".to_string()),
            )
            .await
            {
                Ok(_) => {
                    spawn_butler_task_watcher(app_handle_clone.clone(), task_id);
                    Ok(())
                }
                Err(error) => {
                    record_terminal_task_state(
                        &app_handle_clone,
                        task_id,
                        STATUS_FAILED,
                        format!("任务启动失败：{}", error),
                        format!("任务启动失败：{}", error),
                        None,
                        false,
                    )
                    .await?;
                    Err(error.to_string())
                }
            }
        });

        if let Err(error) = thread_result {
            warn!(task_conversation_id = task_id, error = %error, "failed to launch butler task");
        }
    });

    Ok(SpawnButlerTaskResponse {
        butler_conversation_id,
        task_conversation_id: task_id,
        title: definition_clone.title,
        status: STATUS_RUNNING.to_string(),
        executor_assistant_id,
        executor_assistant_name,
    })
}

#[tauri::command]
pub async fn load_butler_main_conversation(
    app_handle: tauri::AppHandle,
    reconcile: Option<bool>,
) -> Result<ButlerMainLoadResponse, String> {
    let conversation = load_or_create_butler_main_internal(&app_handle).await?;
    if reconcile.unwrap_or(false) {
        reconcile_butler_tasks_once(&app_handle).await?;
    }
    let model_selection = get_butler_model_selection(&app_handle).await?;
    let tasks = list_butler_tasks_internal(&app_handle, conversation.id).await?;
    Ok(ButlerMainLoadResponse {
        conversation,
        model_id: model_selection.raw_value,
        model_display_name: model_selection.display_name,
        tasks,
    })
}

#[tauri::command]
pub async fn reset_butler_main_conversation(
    app_handle: tauri::AppHandle,
) -> Result<ButlerMainLoadResponse, String> {
    ensure_butler_enabled(&app_handle).await?;
    let butler_assistant = ensure_butler_system_assistant(&app_handle).await?;
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let archived_at = Utc::now();
    let mut previous_main_conversation_id = None;

    if let Some(state) = butler_repo.get_main_state(BUTLER_MAIN_SLOT).map_err(|e| e.to_string())? {
        previous_main_conversation_id = Some(state.butler_conversation_id);
        if let Some(mut current_main) =
            conversation_repo.read(state.butler_conversation_id).map_err(|e| e.to_string())?
        {
            current_main.name =
                build_butler_archive_conversation_name(&current_main.name, &archived_at);
            current_main.conversation_kind = BUTLER_KIND_MAIN_ARCHIVE.to_string();
            current_main.is_hidden_from_normal_chat_list = true;
            current_main.updated_time = archived_at;
            conversation_repo.update(&current_main).map_err(|e| e.to_string())?;
        }
    }

    let new_conversation = conversation_repo
        .create(&build_butler_conversation(
            butler_assistant.id,
            "总管家主会话".to_string(),
            BUTLER_KIND_MAIN,
            true,
            None,
            None,
            None,
        ))
        .map_err(|e| e.to_string())?;

    if let Some(previous_main_conversation_id) = previous_main_conversation_id {
        conversation_repo
            .reassign_parent_butler_conversation(previous_main_conversation_id, new_conversation.id)
            .map_err(|e| e.to_string())?;
        butler_repo
            .reassign_task_definitions(previous_main_conversation_id, new_conversation.id)
            .map_err(|e| e.to_string())?;
        inherit_latest_feishu_target(
            &app_handle,
            previous_main_conversation_id,
            new_conversation.id,
        )?;
    }

    butler_repo
        .upsert_main_state(new_conversation.id, BUTLER_MAIN_SLOT)
        .map_err(|e| e.to_string())?;
    butler_repo.touch_main_state(BUTLER_MAIN_SLOT).map_err(|e| e.to_string())?;

    let model_selection = get_butler_model_selection(&app_handle).await?;
    let tasks = list_butler_tasks_internal(&app_handle, new_conversation.id).await?;
    let _ = app_handle.emit("butler_main_reset", json!({ "conversation_id": new_conversation.id }));

    Ok(ButlerMainLoadResponse {
        conversation: new_conversation,
        model_id: model_selection.raw_value,
        model_display_name: model_selection.display_name,
        tasks,
    })
}

#[tauri::command]
pub async fn list_butler_tasks(
    app_handle: tauri::AppHandle,
    butler_conversation_id: i64,
) -> Result<Vec<ButlerTaskListItem>, String> {
    list_butler_tasks_internal(&app_handle, butler_conversation_id).await
}

#[tauri::command]
pub async fn get_butler_task_detail(
    app_handle: tauri::AppHandle,
    task_conversation_id: i64,
) -> Result<ButlerTaskDetailResponse, String> {
    let db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let conversation_repo = db.conversation_repo().map_err(|e| e.to_string())?;
    let butler_repo = db.butler_repo().map_err(|e| e.to_string())?;
    let conversation = conversation_repo
        .read(task_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务会话不存在".to_string())?;
    if conversation.conversation_kind != BUTLER_KIND_TASK {
        return Err("指定会话不是总管家任务会话".to_string());
    }
    let definition = butler_repo
        .get_task_definition_by_task_conversation_id(task_conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务定义不存在".to_string())?;
    let result = butler_repo.get_task_result(task_conversation_id).map_err(|e| e.to_string())?;
    let activity_manager =
        app_handle.state::<crate::state::activity_state::ConversationActivityManager>();
    let runtime_state = activity_manager.get_runtime_state(task_conversation_id).await;
    let task =
        build_task_list_item(&app_handle, conversation.clone(), &definition, result.as_ref())
            .await?;

    Ok(ButlerTaskDetailResponse { task, conversation, definition, result, runtime_state })
}

#[tauri::command]
pub async fn spawn_butler_task_conversation(
    app_handle: tauri::AppHandle,
    window: tauri::Window,
    request: SpawnButlerTaskRequest,
) -> Result<SpawnButlerTaskResponse, String> {
    spawn_butler_task_with_window(&app_handle, &window, request).await
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{
        find_latest_attention_message_id, find_latest_handoff_message_id,
        has_non_system_message_after, parse_task_conversation_id_from_attention_message,
        parse_task_conversation_id_from_handoff_message, try_decide_butler_task_terminal_state,
        update_butler_task_watcher_state, Message, STATUS_CANCELLED, STATUS_FAILED,
        STATUS_SUCCEEDED,
    };

    fn build_message(id: i64, message_type: &str, content: &str, finished: bool) -> Message {
        let now = Utc::now();
        Message {
            id,
            parent_id: None,
            conversation_id: 1,
            message_type: message_type.to_string(),
            content: content.to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: now,
            start_time: Some(now),
            finish_time: finished.then_some(now),
            token_count: 0,
            input_token_count: 0,
            output_token_count: 0,
            generation_group_id: None,
            parent_group_id: None,
            tool_calls_json: None,
            first_token_time: None,
            ttft_ms: None,
        }
    }

    #[test]
    fn cancel_requested_prefers_completed_response() {
        let messages = vec![
            build_message(1, "response", "阶段性输出", false),
            build_message(2, "response", "最终交付物已生成", true),
            build_message(3, "error", "用户请求取消", true),
        ];

        let decision = try_decide_butler_task_terminal_state(&messages, None, true).unwrap();

        assert_eq!(decision.status, STATUS_SUCCEEDED);
        assert_eq!(decision.final_message_id, Some(2));
        assert!(decision.cancel_requested);
        assert!(decision.summary.contains("取消请求后"));
    }

    #[test]
    fn cancel_requested_without_completed_response_stays_cancelled() {
        let messages = vec![
            build_message(1, "response", "未完成的半截输出", false),
            build_message(2, "error", "任务在取消时被中断", true),
        ];

        let decision = try_decide_butler_task_terminal_state(&messages, None, true).unwrap();

        assert_eq!(decision.status, STATUS_CANCELLED);
        assert_eq!(decision.final_message_id, Some(2));
        assert!(decision.cancel_requested);
        assert!(decision.detail_text.contains("取消"));
    }

    #[test]
    fn non_cancelled_task_still_prefers_later_error() {
        let messages = vec![
            build_message(1, "response", "旧回复", true),
            build_message(2, "error", "最终报错", true),
        ];

        let decision = try_decide_butler_task_terminal_state(&messages, None, false).unwrap();

        assert_eq!(decision.status, STATUS_FAILED);
        assert_eq!(decision.final_message_id, Some(2));
        assert!(!decision.cancel_requested);
    }

    #[test]
    fn non_cancelled_task_ignores_tool_call_scaffold_response() {
        let messages = vec![build_message(
            1,
            "response",
            "先查资料\n\n<!-- MCP_TOOL_CALL:{\"call_id\":42} -->",
            true,
        )];

        let decision = try_decide_butler_task_terminal_state(&messages, None, false);

        assert!(decision.is_none());
    }

    #[test]
    fn watcher_attempts_finalization_after_two_idle_polls_once_running_seen() {
        let mut seen_running = false;
        let mut idle_checks = 0usize;

        assert!(!update_butler_task_watcher_state(&mut seen_running, &mut idle_checks, true));
        assert!(seen_running);
        assert_eq!(idle_checks, 0);

        assert!(!update_butler_task_watcher_state(&mut seen_running, &mut idle_checks, false));
        assert_eq!(idle_checks, 1);
        assert!(update_butler_task_watcher_state(&mut seen_running, &mut idle_checks, false));
    }

    #[test]
    fn watcher_does_not_attempt_finalization_before_running_seen() {
        let mut seen_running = false;
        let mut idle_checks = 0usize;

        assert!(!update_butler_task_watcher_state(&mut seen_running, &mut idle_checks, false));
        assert!(!seen_running);
        assert_eq!(idle_checks, 0);
    }

    #[test]
    fn parses_handoff_message_task_id() {
        let content =
            "<butler_task_result>\nstatus=succeeded\ntask_conversation_id=560\n</butler_task_result>";
        assert_eq!(parse_task_conversation_id_from_handoff_message(content), Some(560));
    }

    #[test]
    fn finds_latest_matching_handoff_message() {
        let messages = vec![
            build_message(
                1,
                "system",
                "<butler_task_result>\ntask_conversation_id=12\n</butler_task_result>",
                true,
            ),
            build_message(
                2,
                "system",
                "<butler_task_result>\ntask_conversation_id=42\n</butler_task_result>",
                true,
            ),
            build_message(
                3,
                "system",
                "<butler_task_result>\ntask_conversation_id=42\n</butler_task_result>",
                true,
            ),
        ];

        assert_eq!(find_latest_handoff_message_id(&messages, 42), Some(3));
        assert_eq!(find_latest_handoff_message_id(&messages, 12), Some(1));
    }

    #[test]
    fn detects_pending_followup_when_only_system_messages_exist_after_handoff() {
        let messages = vec![
            build_message(
                10,
                "system",
                "<butler_task_result>\ntask_conversation_id=560\n</butler_task_result>",
                true,
            ),
            build_message(11, "system", "other system", true),
        ];

        assert!(!has_non_system_message_after(&messages, 10));
    }

    #[test]
    fn detects_started_followup_when_non_system_message_exists_after_handoff() {
        let messages = vec![
            build_message(
                10,
                "system",
                "<butler_task_result>\ntask_conversation_id=560\n</butler_task_result>",
                true,
            ),
            build_message(11, "user", "系统任务回流：任务《X》已完成", true),
        ];

        assert!(has_non_system_message_after(&messages, 10));
    }

    #[test]
    fn parses_attention_message_task_id() {
        let content =
            "<butler_task_attention>\nattention_kind=operation\ntask_conversation_id=560\n</butler_task_attention>";
        assert_eq!(parse_task_conversation_id_from_attention_message(content), Some(560));
    }

    #[test]
    fn finds_latest_matching_attention_message() {
        let messages = vec![
            build_message(
                1,
                "system",
                "<butler_task_attention>\ntask_conversation_id=12\n</butler_task_attention>",
                true,
            ),
            build_message(
                2,
                "system",
                "<butler_task_attention>\ntask_conversation_id=42\n</butler_task_attention>",
                true,
            ),
            build_message(
                3,
                "system",
                "<butler_task_attention>\ntask_conversation_id=42\n</butler_task_attention>",
                true,
            ),
        ];

        assert_eq!(find_latest_attention_message_id(&messages, 42), Some(3));
        assert_eq!(find_latest_attention_message_id(&messages, 12), Some(1));
    }
}
