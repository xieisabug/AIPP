use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{Emitter, State};
use tracing::{error, instrument};

use crate::api::ai::chat::handle_non_stream_chat as ai_handle_non_stream_chat;
use crate::api::ai::config::{get_network_proxy_from_config, get_request_timeout_from_config, ConfigBuilder};
use crate::api::ai::conversation::{build_chat_request_from_messages, ToolCallStrategy, ToolConfig};
use crate::api::ai_api::build_tools_with_mapping;
use crate::api::assistant_api::get_assistant;
use crate::api::genai_client::create_client_with_config;
use crate::db::assistant_db::AssistantDatabase;
use crate::db::conversation_db::{ConversationDatabase, Repository as ConversationRepository};
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::MCPDatabase;
use crate::db::scheduled_task_db::{ScheduledTask, ScheduledTaskDatabase, ScheduledTaskLog, ScheduledTaskRun};
use crate::db::system_db::FeatureConfig;
use crate::mcp::{collect_mcp_info_for_assistant, format_mcp_prompt, MCPInfoForAssistant};
use crate::skills::{collect_skills_info_for_assistant, format_skills_prompt};
use crate::template_engine::TemplateEngine;
use crate::{AppState, FeatureConfigState, NameCacheState};
use genai::chat::ChatOptions;
use tauri::Manager;
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskDTO {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String,
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>,
    pub run_at: Option<String>,
    pub next_run_at: Option<String>,
    pub last_run_at: Option<String>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
    pub created_time: String,
    pub updated_time: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduledTaskRequest {
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String, // 'once' | 'interval'
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>, // minute/hour/day/week/month
    pub run_at: Option<String>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UpdateScheduledTaskRequest {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
    pub schedule_type: String,
    pub interval_value: Option<i64>,
    pub interval_unit: Option<String>,
    pub run_at: Option<String>,
    pub assistant_id: i64,
    pub task_prompt: String,
    pub notify_prompt: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct RunScheduledTaskResult {
    pub task_id: i64,
    pub success: bool,
    pub notify: bool,
    pub summary: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskLogDTO {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub message_type: String,
    pub content: String,
    pub created_time: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListScheduledTaskLogsResponse {
    pub logs: Vec<ScheduledTaskLogDTO>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTaskRunDTO {
    pub id: i64,
    pub task_id: i64,
    pub run_id: String,
    pub status: String,
    pub notify: bool,
    pub summary: Option<String>,
    pub error_message: Option<String>,
    pub started_time: String,
    pub finished_time: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ListScheduledTaskRunsResponse {
    pub runs: Vec<ScheduledTaskRunDTO>,
}

fn format_dt(dt: Option<DateTime<Utc>>) -> Option<String> {
    dt.map(|v| v.to_rfc3339())
}

fn parse_local_datetime(input: &str) -> Result<DateTime<Utc>, String> {
    let trimmed = input.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Ok(dt.with_timezone(&Utc));
    }
    let formats = [
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
    ];
    for fmt in &formats {
        if let Ok(naive) = NaiveDateTime::parse_from_str(trimmed, fmt) {
            let local_dt = Local
                .from_local_datetime(&naive)
                .single()
                .ok_or_else(|| "无法解析本地时间".to_string())?;
            return Ok(local_dt.with_timezone(&Utc));
        }
    }
    Err("无法解析时间，请使用 YYYY-MM-DD HH:MM:SS 格式".to_string())
}

fn validate_assistant_type(app_handle: &tauri::AppHandle, assistant_id: i64) -> Result<(), String> {
    let db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let assistant = db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    if assistant.assistant_type.unwrap_or(0) != 0 {
        return Err("只能选择普通对话助手".to_string());
    }
    Ok(())
}

fn resolve_ai_window(app_handle: &tauri::AppHandle) -> Result<tauri::Window, String> {
    if let Some(window) = app_handle.get_webview_window("chat_ui") {
        return Ok(window.as_ref().window());
    }
    if let Some(window) = app_handle.get_webview_window("ask") {
        return Ok(window.as_ref().window());
    }
    if let Some(window) = app_handle.get_webview_window("schedule") {
        return Ok(window.as_ref().window());
    }
    Err("未找到可用的聊天窗口".to_string())
}

fn build_tool_config_for_mcp(mcp_info: &MCPInfoForAssistant, enable_tools: bool) -> Option<ToolConfig> {
    if !enable_tools {
        return None;
    }
    let (tools, tool_name_mapping) = build_tools_with_mapping(&mcp_info.enabled_servers);
    Some(ToolConfig { tools, tool_name_mapping })
}

fn suppress_completion_notification(
    config_feature_map: &mut HashMap<String, HashMap<String, FeatureConfig>>,
) {
    let display_config = config_feature_map
        .entry("display".to_string())
        .or_insert_with(HashMap::new);
    display_config.insert(
        "notification_on_completion".to_string(),
        FeatureConfig {
            id: None,
            feature_code: "display".to_string(),
            key: "notification_on_completion".to_string(),
            value: "false".to_string(),
            data_type: "string".to_string(),
            description: Some("scheduled task override".to_string()),
        },
    );
}

fn write_task_log(
    app_handle: &tauri::AppHandle,
    task_id: i64,
    run_id: &str,
    message_type: &str,
    content: impl Into<String>,
) -> Result<(), String> {
    let db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let log = ScheduledTaskLog {
        id: 0,
        task_id,
        run_id: run_id.to_string(),
        message_type: message_type.to_string(),
        content: content.into(),
        created_time: Utc::now(),
    };
    db.add_log(&log).map(|_| ()).map_err(|e| e.to_string())
}

fn log_task_message(
    app_handle: &tauri::AppHandle,
    task_id: i64,
    run_id: &str,
    message_type: &str,
    content: impl Into<String>,
) {
    let content = content.into();
    let _ = write_task_log(app_handle, task_id, run_id, message_type, content);
}

fn update_task_run(
    app_handle: &tauri::AppHandle,
    run_id: &str,
    status: &str,
    notify: bool,
    summary: Option<&str>,
    error_message: Option<&str>,
    finished_time: Option<DateTime<Utc>>,
) {
    let _ = ScheduledTaskDatabase::new(app_handle)
        .map_err(|e| e.to_string())
        .and_then(|db| {
            db.update_run_result(run_id, status, notify, summary, error_message, finished_time)
                .map_err(|e| e.to_string())
        });
}

fn cleanup_conversation(app_handle: &tauri::AppHandle, conversation_id: i64) -> Result<(), String> {
    let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
    let conn = conversation_db.get_connection().map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM message_attachment WHERE message_id IN (SELECT id FROM message WHERE conversation_id = ?)",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM message WHERE conversation_id = ?",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM conversation_summary WHERE conversation_id = ?",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;
    conn.execute(
        "DELETE FROM conversation WHERE id = ?",
        params![conversation_id],
    )
    .map_err(|e| e.to_string())?;

    if let Ok(mcp_db) = MCPDatabase::new(app_handle) {
        let _ = mcp_db
            .conn
            .execute("DELETE FROM mcp_tool_call WHERE conversation_id = ?", params![conversation_id]);
    }

    Ok(())
}

pub fn compute_next_run_at(
    schedule_type: &str,
    interval_value: Option<i64>,
    interval_unit: Option<&str>,
    run_at: Option<DateTime<Utc>>,
    base_time: DateTime<Utc>,
) -> Result<Option<DateTime<Utc>>, String> {
    if schedule_type == "once" {
        return Ok(run_at);
    }
    if schedule_type != "interval" {
        return Err("不支持的 schedule_type".to_string());
    }
    let value = interval_value.ok_or_else(|| "缺少 interval_value".to_string())?;
    let unit = interval_unit.ok_or_else(|| "缺少 interval_unit".to_string())?;
    if value <= 0 {
        return Err("interval_value 需要大于 0".to_string());
    }
    let add_interval = |current: DateTime<Utc>| -> Result<DateTime<Utc>, String> {
        match unit {
            "minute" => Ok(current + chrono::Duration::minutes(value)),
            "hour" => Ok(current + chrono::Duration::hours(value)),
            "day" => Ok(current + chrono::Duration::days(value)),
            "week" => Ok(current + chrono::Duration::days(value * 7)),
            "month" => {
                let months = chrono::Months::new(value as u32);
                current
                    .checked_add_months(months)
                    .ok_or_else(|| "无法计算下次执行时间".to_string())
            }
            _ => Err("不支持的 interval_unit".to_string()),
        }
    };

    let start = match run_at {
        Some(value) => value,
        None => add_interval(base_time)?,
    };

    if base_time <= start {
        return Ok(Some(start));
    }

    if unit == "month" {
        let mut next = start;
        for _ in 0..240 {
            next = add_interval(next)?;
            if next > base_time {
                return Ok(Some(next));
            }
        }
        return Err("无法计算下次执行时间".to_string());
    }

    let interval_seconds = match unit {
        "minute" => value * 60,
        "hour" => value * 60 * 60,
        "day" => value * 60 * 60 * 24,
        "week" => value * 60 * 60 * 24 * 7,
        _ => return Err("不支持的 interval_unit".to_string()),
    };
    if interval_seconds <= 0 {
        return Err("interval_value 需要大于 0".to_string());
    }
    let elapsed = (base_time - start).num_seconds();
    let intervals = elapsed / interval_seconds + 1;
    let next = start + chrono::Duration::seconds(interval_seconds * intervals);
    Ok(Some(next))
}

fn to_dto(task: ScheduledTask) -> ScheduledTaskDTO {
    ScheduledTaskDTO {
        id: task.id,
        name: task.name,
        is_enabled: task.is_enabled,
        schedule_type: task.schedule_type,
        interval_value: task.interval_value,
        interval_unit: task.interval_unit,
        run_at: format_dt(task.run_at),
        next_run_at: format_dt(task.next_run_at),
        last_run_at: format_dt(task.last_run_at),
        assistant_id: task.assistant_id,
        task_prompt: task.task_prompt,
        notify_prompt: task.notify_prompt,
        created_time: task.created_time.to_rfc3339(),
        updated_time: task.updated_time.to_rfc3339(),
    }
}

fn log_to_dto(log: ScheduledTaskLog) -> ScheduledTaskLogDTO {
    ScheduledTaskLogDTO {
        id: log.id,
        task_id: log.task_id,
        run_id: log.run_id,
        message_type: log.message_type,
        content: log.content,
        created_time: log.created_time.to_rfc3339(),
    }
}

fn run_to_dto(run: ScheduledTaskRun) -> ScheduledTaskRunDTO {
    ScheduledTaskRunDTO {
        id: run.id,
        task_id: run.task_id,
        run_id: run.run_id,
        status: run.status,
        notify: run.notify,
        summary: run.summary,
        error_message: run.error_message,
        started_time: run.started_time.to_rfc3339(),
        finished_time: run.finished_time.map(|v| v.to_rfc3339()),
    }
}

#[tauri::command]
pub async fn list_scheduled_tasks(app_handle: tauri::AppHandle) -> Result<Vec<ScheduledTaskDTO>, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let tasks = db.list_tasks().map_err(|e| e.to_string())?;
    Ok(tasks.into_iter().map(to_dto).collect())
}

#[tauri::command]
pub async fn list_scheduled_task_logs(
    app_handle: tauri::AppHandle,
    task_id: i64,
    run_id: Option<String>,
    limit: Option<u32>,
) -> Result<ListScheduledTaskLogsResponse, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let limit_value = limit.unwrap_or(200).min(1000);
    let logs = match run_id {
        Some(run_id) => db
            .list_logs_by_run(task_id, &run_id, limit_value)
            .map_err(|e| e.to_string())?,
        None => db
            .list_logs_by_task(task_id, limit_value)
            .map_err(|e| e.to_string())?,
    };
    Ok(ListScheduledTaskLogsResponse {
        logs: logs.into_iter().map(log_to_dto).collect(),
    })
}

#[tauri::command]
pub async fn list_scheduled_task_runs(
    app_handle: tauri::AppHandle,
    task_id: i64,
    limit: Option<u32>,
) -> Result<ListScheduledTaskRunsResponse, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let limit_value = limit.unwrap_or(50).min(200);
    let runs = db
        .list_runs_by_task(task_id, limit_value)
        .map_err(|e| e.to_string())?;
    Ok(ListScheduledTaskRunsResponse {
        runs: runs.into_iter().map(run_to_dto).collect(),
    })
}

#[tauri::command]
pub async fn create_scheduled_task(
    app_handle: tauri::AppHandle,
    request: CreateScheduledTaskRequest,
) -> Result<ScheduledTaskDTO, String> {
    validate_assistant_type(&app_handle, request.assistant_id)?;
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let now = Utc::now();
    let run_at = match &request.run_at {
        Some(value) => Some(parse_local_datetime(value)?),
        None => None,
    };
    if request.schedule_type == "once" && run_at.is_none() {
        return Err("一次性任务需要设置执行时间".to_string());
    }
    let next_run_at = compute_next_run_at(
        &request.schedule_type,
        request.interval_value,
        request.interval_unit.as_deref(),
        run_at,
        now,
    )?;

    let task = ScheduledTask {
        id: 0,
        name: request.name,
        is_enabled: request.is_enabled,
        schedule_type: request.schedule_type,
        interval_value: request.interval_value,
        interval_unit: request.interval_unit,
        run_at,
        next_run_at,
        last_run_at: None,
        assistant_id: request.assistant_id,
        task_prompt: request.task_prompt,
        notify_prompt: request.notify_prompt,
        created_time: now,
        updated_time: now,
    };
    let created = db.create_task(&task).map_err(|e| e.to_string())?;
    Ok(to_dto(created))
}

#[tauri::command]
pub async fn update_scheduled_task(
    app_handle: tauri::AppHandle,
    request: UpdateScheduledTaskRequest,
) -> Result<ScheduledTaskDTO, String> {
    validate_assistant_type(&app_handle, request.assistant_id)?;
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let existing = db
        .read_task(request.id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在".to_string())?;
    let now = Utc::now();
    let run_at = match &request.run_at {
        Some(value) => Some(parse_local_datetime(value)?),
        None => None,
    };
    if request.schedule_type == "once" && run_at.is_none() {
        return Err("一次性任务需要设置执行时间".to_string());
    }
    let next_run_at = compute_next_run_at(
        &request.schedule_type,
        request.interval_value,
        request.interval_unit.as_deref(),
        run_at,
        now,
    )?;
    let updated = ScheduledTask {
        id: existing.id,
        name: request.name,
        is_enabled: request.is_enabled,
        schedule_type: request.schedule_type,
        interval_value: request.interval_value,
        interval_unit: request.interval_unit,
        run_at,
        next_run_at,
        last_run_at: existing.last_run_at,
        assistant_id: request.assistant_id,
        task_prompt: request.task_prompt,
        notify_prompt: request.notify_prompt,
        created_time: existing.created_time,
        updated_time: now,
    };
    db.update_task(&updated).map_err(|e| e.to_string())?;
    Ok(to_dto(updated))
}

#[tauri::command]
pub async fn delete_scheduled_task(app_handle: tauri::AppHandle, task_id: i64) -> Result<(), String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    db.delete_task(task_id).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn run_scheduled_task_now(
    app_handle: tauri::AppHandle,
    feature_config_state: State<'_, FeatureConfigState>,
    task_id: i64,
) -> Result<RunScheduledTaskResult, String> {
    let db = ScheduledTaskDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let task = db
        .read_task(task_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "任务不存在".to_string())?;
    let now = Utc::now();
    let next_run_at = if task.schedule_type == "once" {
        None
    } else {
        compute_next_run_at(
            &task.schedule_type,
            task.interval_value,
            task.interval_unit.as_deref(),
            task.run_at,
            now,
        )?
    };
    let updated = ScheduledTask {
        is_enabled: if task.schedule_type == "once" {
            false
        } else {
            task.is_enabled
        },
        last_run_at: Some(now),
        next_run_at,
        updated_time: now,
        ..task.clone()
    };
    db.update_task(&updated).map_err(|e| e.to_string())?;

    match execute_scheduled_task(&app_handle, &feature_config_state, &updated).await {
        Ok(result) => Ok(result),
        Err(e) => Ok(RunScheduledTaskResult {
            task_id,
            success: false,
            notify: false,
            summary: None,
            error: Some(e),
        }),
    }
}

pub async fn execute_scheduled_task(
    app_handle: &tauri::AppHandle,
    feature_config_state: &FeatureConfigState,
    task: &ScheduledTask,
) -> Result<RunScheduledTaskResult, String> {
    let run_id = Uuid::new_v4().to_string();
    let run_started_at = Utc::now();
    {
        let log_db = ScheduledTaskDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let _ = log_db.create_run(&ScheduledTaskRun {
            id: 0,
            task_id: task.id,
            run_id: run_id.clone(),
            status: "running".to_string(),
            notify: false,
            summary: None,
            error_message: None,
            started_time: run_started_at,
            finished_time: None,
        });
    }
    log_task_message(app_handle, task.id, &run_id, "start", "开始执行定时任务");
    let run_result = (|| async {
        let assistant_detail = get_assistant(app_handle.clone(), task.assistant_id)
            .map_err(|e| format!("Failed to get assistant: {}", e))?;
        if assistant_detail.assistant.assistant_type.unwrap_or(0) != 0 {
            log_task_message(
                app_handle,
                task.id,
                &run_id,
                "error",
                "只能选择普通对话助手",
            );
            update_task_run(
                app_handle,
                &run_id,
                "failed",
                false,
                None,
                Some("只能选择普通对话助手"),
                Some(Utc::now()),
            );
            return Err("只能选择普通对话助手".to_string());
        }
        if assistant_detail.model.is_empty() {
            log_task_message(app_handle, task.id, &run_id, "error", "助手未配置模型");
            update_task_run(
                app_handle,
                &run_id,
                "failed",
                false,
                None,
                Some("助手未配置模型"),
                Some(Utc::now()),
            );
            return Err("助手未配置模型".to_string());
        }
        let system_prompt = assistant_detail
            .prompts
            .get(0)
            .map(|p| p.prompt.clone())
            .ok_or_else(|| "助手未配置系统提示词".to_string())?;

        let template_engine = TemplateEngine::new();
        let mut template_context = HashMap::new();
        if let Some(state) = app_handle.try_state::<AppState>() {
            let selected_text = state.selected_text.lock().await.clone();
            template_context.insert("selected_text".to_string(), selected_text);
        }

        let system_prompt_rendered = template_engine.parse(&system_prompt, &template_context).await;
        let task_prompt_rendered = template_engine.parse(&task.task_prompt, &template_context).await;
        log_task_message(
            app_handle,
            task.id,
            &run_id,
            "task_prompt",
            task_prompt_rendered.clone(),
        );

        let mcp_info = collect_mcp_info_for_assistant(app_handle, task.assistant_id, None, None)
            .await
            .map_err(|e| e.to_string())?;
        let mut assistant_prompt_result = if !mcp_info.enabled_servers.is_empty() && !mcp_info.use_native_toolcall {
            format_mcp_prompt(system_prompt_rendered.clone(), &mcp_info).await
        } else {
            system_prompt_rendered.clone()
        };
        let skills_info = collect_skills_info_for_assistant(app_handle, task.assistant_id)
            .await
            .map_err(|e| e.to_string())?;
        if !skills_info.enabled_skills.is_empty() {
            assistant_prompt_result =
                format_skills_prompt(app_handle, assistant_prompt_result, &skills_info).await;
        }

        let llm_db = LLMDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let model_info = &assistant_detail.model[0];
        let model_detail = llm_db
            .get_llm_model_detail(&model_info.provider_id, &model_info.model_code)
            .map_err(|e| format!("Failed to get LLM model: {}", e))?;

        let mut config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
        suppress_completion_notification(&mut config_feature_map);
        let network_proxy = get_network_proxy_from_config(&config_feature_map);
        let request_timeout = get_request_timeout_from_config(&config_feature_map);
        let proxy_enabled = model_detail
            .configs
            .iter()
            .find(|config| config.name == "proxy_enabled")
            .and_then(|config| config.value.parse::<bool>().ok())
            .unwrap_or(false);

        let client = create_client_with_config(
            &model_detail.configs,
            &model_detail.model.code,
            &model_detail.provider.api_type,
            network_proxy.as_deref(),
            proxy_enabled,
            Some(request_timeout),
        )
        .map_err(|e| format!("Failed to create AI client: {}", e))?;
        log_task_message(
            app_handle,
            task.id,
            &run_id,
            "assistant",
            format!(
                "助手: {} / 模型: {}",
                assistant_detail.assistant.name, model_detail.model.code
            ),
        );

        let model_config_clone =
            ConfigBuilder::merge_model_configs(assistant_detail.model_configs.clone(), &model_detail, None);
        let config_map = model_config_clone
            .iter()
            .filter_map(|config| config.value.as_ref().map(|value| (config.name.clone(), value.clone())))
            .collect::<HashMap<String, String>>();
        let model_name = config_map
            .get("model")
            .cloned()
            .unwrap_or_else(|| model_detail.model.code.clone());
        let provider_api_type = model_detail.provider.api_type.clone();
        let provider_api_type_lc = provider_api_type.to_lowercase();
        let model_code_lc = model_detail.model.code.to_lowercase();
        let is_openai_like = provider_api_type_lc == "openai" || provider_api_type_lc == "openai_api";
        let is_gemini = model_code_lc.contains("gemini");
        let capture_usage = !(is_openai_like && is_gemini);
        let has_available_tools = mcp_info.use_native_toolcall && !mcp_info.enabled_servers.is_empty();
        let mut chat_options = ConfigBuilder::build_chat_options(&config_map);
        chat_options = chat_options
            .with_normalize_reasoning_content(true)
            .with_capture_usage(capture_usage)
            .with_capture_tool_calls(has_available_tools);

        let tool_call_strategy = if has_available_tools {
            ToolCallStrategy::Native
        } else {
            ToolCallStrategy::NonNative
        };
        let tool_config = build_tool_config_for_mcp(&mcp_info, has_available_tools);
        let init_messages = vec![
            ("system".to_string(), assistant_prompt_result.clone(), Vec::new()),
            ("user".to_string(), task_prompt_rendered.clone(), Vec::new()),
        ];
        let (mut conversation, _) = crate::api::ai::conversation::init_conversation(
            app_handle,
            task.assistant_id,
            model_detail.model.id,
            model_detail.model.code.clone(),
            &init_messages,
        )
        .map_err(|e| e.to_string())?;
        let conversation_id = conversation.id;
        let window = resolve_ai_window(app_handle)?;
        let conversation_db = ConversationDatabase::new(app_handle).map_err(|e| e.to_string())?;
        let task_request_result =
            build_chat_request_from_messages(&init_messages, tool_call_strategy, tool_config);

        if let Err(e) = ai_handle_non_stream_chat(
            &client,
            &model_name,
            &task_request_result.chat_request,
            &chat_options,
            conversation_id,
            &conversation_db,
            &window,
            app_handle,
            false,
            task_prompt_rendered.clone(),
            config_feature_map.clone(),
            None,
            None,
            model_detail.model.id,
            model_detail.model.code.clone(),
            None,
            task_request_result.tool_name_mapping,
        )
        .await
        {
            log_task_message(
                app_handle,
                task.id,
                &run_id,
                "error",
                format!("任务执行失败: {}", e),
            );
            update_task_run(
                app_handle,
                &run_id,
                "failed",
                false,
                None,
                Some(&format!("任务执行失败: {}", e)),
                Some(Utc::now()),
            );
            let _ = cleanup_conversation(app_handle, conversation_id);
            return Err(format!("任务执行失败: {}", e));
        }

        let all_messages = conversation_db
            .message_repo()
            .map_err(|e| e.to_string())?
            .list_by_conversation_id(conversation_id)
            .map_err(|e| e.to_string())?;
        let task_result = all_messages
            .iter()
            .rev()
            .find(|(msg, _)| msg.message_type == "response")
            .map(|(msg, _)| msg.content.clone())
            .unwrap_or_default();
        if task_result.is_empty() {
            log_task_message(
                app_handle,
                task.id,
                &run_id,
                "response",
                "任务执行未返回内容",
            );
        } else {
            log_task_message(app_handle, task.id, &run_id, "response", task_result.clone());
        }

        let notify_request = build_chat_request_from_messages(
            &[
                ("system".to_string(), system_prompt_rendered.clone(), Vec::new()),
                ("user".to_string(), task.notify_prompt.clone(), Vec::new()),
                ("assistant".to_string(), task_result.clone(), Vec::new()),
            ],
            ToolCallStrategy::NonNative,
            None,
        )
        .chat_request;

        let notify_options = chat_options.clone().with_capture_tool_calls(false);
        let notify_response = client
            .exec_chat(&model_name, notify_request, Some(&notify_options))
            .await
            .map_err(|e| {
                log_task_message(
                    app_handle,
                    task.id,
                    &run_id,
                    "error",
                    format!("通知判定执行失败: {}", e),
                );
                update_task_run(
                    app_handle,
                    &run_id,
                    "failed",
                    false,
                    None,
                    Some(&format!("通知判定执行失败: {}", e)),
                    Some(Utc::now()),
                );
                format!("通知判定执行失败: {}", e)
            })?;
        let notify_text = notify_response.content.into_joined_texts().unwrap_or_default();
        log_task_message(app_handle, task.id, &run_id, "notify_raw", notify_text.clone());

        let notify_json: serde_json::Value = serde_json::from_str(&notify_text).map_err(|e| {
            error!(error = %e, raw = %notify_text, "invalid notify JSON");
            log_task_message(
                app_handle,
                task.id,
                &run_id,
                "error",
                format!("通知判定返回格式不正确: {}", e),
            );
            update_task_run(
                app_handle,
                &run_id,
                "failed",
                false,
                None,
                Some(&format!("通知判定返回格式不正确: {}", e)),
                Some(Utc::now()),
            );
            "通知判定返回格式不正确，需为 JSON".to_string()
        })?;
        let notify = notify_json
            .get("notify")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let summary = notify_json
            .get("summary")
            .and_then(|v| v.as_str())
            .map(|v| v.to_string());
        log_task_message(
            app_handle,
            task.id,
            &run_id,
            "notify_result",
            format!(
                "notify={}, summary={}",
                notify,
                summary.clone().unwrap_or_default()
            ),
        );

        if notify {
            conversation.name = format!("计划任务: {}", task.name);
            let _ = conversation_db
                .conversation_repo()
                .map_err(|e| e.to_string())?
                .update(&conversation);
            if let Some(cache_state) = app_handle.try_state::<NameCacheState>() {
                let mut assistant_name_cache = cache_state.assistant_names.lock().await;
                assistant_name_cache.insert(task.assistant_id, assistant_detail.assistant.name.clone());
            }
            let _ = app_handle.emit("conversation_created", conversation.id);

            let _ = app_handle
                .notification()
                .builder()
                .title("定时任务完成")
                .body("定时任务完成，请查看对话")
                .show();
            log_task_message(
                app_handle,
                task.id,
                &run_id,
                "notify",
                "已发送系统通知",
            );
            update_task_run(
                app_handle,
                &run_id,
                "success",
                notify,
                summary.as_deref(),
                None,
                Some(Utc::now()),
            );
        } else {
            cleanup_conversation(app_handle, conversation_id)?;
            log_task_message(
                app_handle,
                task.id,
                &run_id,
                "cleanup",
                "未通知，已清理对话记录",
            );
            update_task_run(
                app_handle,
                &run_id,
                "success",
                notify,
                summary.as_deref(),
                None,
                Some(Utc::now()),
            );
        }

        Ok(RunScheduledTaskResult {
            task_id: task.id,
            success: true,
            notify,
            summary: if notify { summary } else { None },
            error: None,
        })
    })()
    .await;

    if let Err(err) = &run_result {
        update_task_run(
            app_handle,
            &run_id,
            "failed",
            false,
            None,
            Some(err),
            Some(Utc::now()),
        );
    }
    run_result
}
