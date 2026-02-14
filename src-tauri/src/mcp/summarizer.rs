use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config,
};
use crate::api::genai_client;
use crate::db::llm_db::LLMDatabase;
use crate::db::mcp_db::{MCPDatabase, MCPServer, MCPServerTool};
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

static MCP_SUMMARY_RUNNING: AtomicBool = AtomicBool::new(false);

const MCP_SUMMARIZER_SYSTEM_PROMPT: &str = r#"
你是 MCP 工具目录摘要助手。请根据输入生成精简摘要并严格输出 JSON。

要求：
1. server_summary：一句话概括服务器能力，中文，<=40字。
2. tool_summaries：数组，每项包含 tool_name 与 summary。
3. 每个工具 summary 中文，<=30字，聚焦用途。
4. 只输出 JSON，不要 Markdown，不要额外解释。

输出格式：
{
  "server_summary": "......",
  "tool_summaries": [
    {"tool_name":"tool_a","summary":"......"}
  ]
}
"#;

#[derive(Debug, Clone, Serialize)]
struct MCPSummaryProgressPayload {
    phase: String,
    total: usize,
    completed: usize,
    succeeded: usize,
    failed: usize,
    server_name: Option<String>,
    message: Option<String>,
}

fn emit_summary_progress(app_handle: &tauri::AppHandle, payload: MCPSummaryProgressPayload) {
    let _ = app_handle.emit("mcp-summary-progress", payload);
}

fn parse_bool(value: &str, default_value: bool) -> bool {
    let value = value.trim().to_lowercase();
    if value.is_empty() {
        return default_value;
    }
    !(value == "false" || value == "0" || value == "off")
}

fn truncate_text(input: &str, max_len: usize) -> String {
    if input.chars().count() <= max_len {
        return input.to_string();
    }
    input.chars().take(max_len).collect::<String>()
}

fn build_summary_user_prompt(server: &MCPServer, tools: &[MCPServerTool]) -> String {
    let tool_entries: Vec<serde_json::Value> = tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "tool_name": tool.tool_name,
                "description": tool.tool_description.clone().unwrap_or_default(),
                "parameters": truncate_text(tool.parameters.as_deref().unwrap_or("{}"), 1200),
            })
        })
        .collect();

    serde_json::json!({
        "server_name": server.name,
        "server_description": server.description,
        "tools": tool_entries
    })
    .to_string()
}

fn extract_json_payload(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let without_fence = if trimmed.starts_with("```") {
        let mut lines = trimmed.lines().collect::<Vec<_>>();
        if !lines.is_empty() {
            lines.remove(0);
        }
        if !lines.is_empty() && lines.last().map(|line| line.trim()) == Some("```") {
            lines.pop();
        }
        lines.join("\n")
    } else {
        trimmed.to_string()
    };

    if serde_json::from_str::<serde_json::Value>(&without_fence).is_ok() {
        return Some(without_fence);
    }

    let start = without_fence.find('{')?;
    let end = without_fence.rfind('}')?;
    if end < start {
        return None;
    }
    Some(without_fence[start..=end].to_string())
}

fn parse_summary_response(raw: &str) -> Option<(Option<String>, HashMap<String, String>)> {
    let payload = extract_json_payload(raw)?;
    let value = serde_json::from_str::<serde_json::Value>(&payload).ok()?;

    let server_summary = value
        .get("server_summary")
        .and_then(|v| v.as_str())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    let mut tool_summaries = HashMap::new();
    if let Some(items) = value
        .get("tool_summaries")
        .or_else(|| value.get("tools"))
        .and_then(|v| v.as_array())
    {
        for item in items {
            let tool_name = item
                .get("tool_name")
                .or_else(|| item.get("name"))
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_lowercase());
            let summary = item
                .get("summary")
                .and_then(|v| v.as_str())
                .map(|v| v.trim().to_string());
            if let (Some(tool_name), Some(summary)) = (tool_name, summary) {
                if !tool_name.is_empty() && !summary.is_empty() {
                    tool_summaries.insert(tool_name, summary);
                }
            }
        }
    }

    Some((server_summary, tool_summaries))
}

fn parse_model_selection(
    config_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Option<(String, i64)> {
    let experimental = config_map.get("experimental")?;
    let dynamic_enabled = experimental
        .get("dynamic_mcp_loading_enabled")
        .map(|cfg| parse_bool(&cfg.value, false))
        .unwrap_or(false);
    if !dynamic_enabled {
        return None;
    }

    let raw = experimental.get("mcp_summarizer_model_id")?.value.trim().to_string();
    if raw.is_empty() {
        return None;
    }

    let parts: Vec<&str> = raw.split("%%").collect();
    if parts.len() != 2 {
        warn!(value = %raw, "Invalid mcp_summarizer_model_id format");
        return None;
    }

    let model_code = parts[0].trim().to_string();
    let provider_id = parts[1].trim().parse::<i64>().ok();
    match (model_code.is_empty(), provider_id) {
        (false, Some(provider_id)) => Some((model_code, provider_id)),
        _ => {
            warn!(value = %raw, "Invalid mcp_summarizer_model_id value");
            None
        }
    }
}

async fn get_feature_config_map(
    app_handle: &tauri::AppHandle,
) -> Result<HashMap<String, HashMap<String, FeatureConfig>>, AppError> {
    let feature_state = app_handle
        .try_state::<crate::FeatureConfigState>()
        .ok_or_else(|| AppError::UnknownError("无法获取功能配置状态".to_string()))?;
    let config_map = feature_state.config_feature_map.lock().await.clone();
    Ok(config_map)
}

#[tauri::command]
pub async fn summarize_all_mcp_catalogs(app_handle: tauri::AppHandle) -> Result<(), String> {
    if MCP_SUMMARY_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("MCP 总结任务正在进行中，请稍后重试".to_string());
    }
    struct ResetRunning;
    impl Drop for ResetRunning {
        fn drop(&mut self) {
            MCP_SUMMARY_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _reset = ResetRunning;

    let config_map = get_feature_config_map(&app_handle).await.map_err(|e| e.to_string())?;
    if parse_model_selection(&config_map).is_none() {
        return Err("请先在实验性配置中选择 MCP 总结 AI 模型".to_string());
    }

    let db = MCPDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let _ = db.rebuild_dynamic_mcp_catalog();
    let servers: Vec<MCPServer> = db
        .get_mcp_servers()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|server| server.is_enabled && server.command.as_deref() != Some("aipp:dynamic_mcp"))
        .collect();

    let total = servers.len();
    let mut completed = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    emit_summary_progress(
        &app_handle,
        MCPSummaryProgressPayload {
            phase: "started".to_string(),
            total,
            completed,
            succeeded,
            failed,
            server_name: None,
            message: Some("开始总结 MCP 目录".to_string()),
        },
    );

    for server in servers {
        emit_summary_progress(
            &app_handle,
            MCPSummaryProgressPayload {
                phase: "processing".to_string(),
                total,
                completed,
                succeeded,
                failed,
                server_name: Some(server.name.clone()),
                message: Some("正在总结".to_string()),
            },
        );

        let result = generate_mcp_catalog_summary(&app_handle, server.id).await;
        completed += 1;
        match result {
            Ok(()) => {
                succeeded += 1;
                emit_summary_progress(
                    &app_handle,
                    MCPSummaryProgressPayload {
                        phase: "progress".to_string(),
                        total,
                        completed,
                        succeeded,
                        failed,
                        server_name: Some(server.name.clone()),
                        message: Some("总结完成".to_string()),
                    },
                );
            }
            Err(err) => {
                failed += 1;
                warn!(server_id = server.id, server_name = %server.name, error = %err, "MCP summary failed");
                emit_summary_progress(
                    &app_handle,
                    MCPSummaryProgressPayload {
                        phase: "progress".to_string(),
                        total,
                        completed,
                        succeeded,
                        failed,
                        server_name: Some(server.name.clone()),
                        message: Some(format!("总结失败: {}", err)),
                    },
                );
            }
        }
    }

    emit_summary_progress(
        &app_handle,
        MCPSummaryProgressPayload {
            phase: "completed".to_string(),
            total,
            completed,
            succeeded,
            failed,
            server_name: None,
            message: Some("MCP 总结任务完成".to_string()),
        },
    );

    Ok(())
}

pub fn trigger_mcp_catalog_summary_generation(app_handle: tauri::AppHandle, server_id: i64) {
    tokio::spawn(async move {
        if let Err(err) = generate_mcp_catalog_summary(&app_handle, server_id).await {
            warn!(server_id, error = %err, "MCP summary generation failed");
        }
    });
}

async fn generate_mcp_catalog_summary(
    app_handle: &tauri::AppHandle,
    server_id: i64,
) -> Result<(), AppError> {
    let config_map = get_feature_config_map(app_handle).await?;
    let Some((model_code, provider_id)) = parse_model_selection(&config_map) else {
        debug!(server_id, "MCP summarizer model not configured, skip generation");
        return Ok(());
    };

    let mcp_db = MCPDatabase::new(app_handle)?;
    let server = mcp_db.get_mcp_server(server_id)?;
    if !server.is_enabled || server.command.as_deref() == Some("aipp:dynamic_mcp") {
        return Ok(());
    }

    let tools: Vec<MCPServerTool> = mcp_db
        .get_mcp_server_tools(server_id)?
        .into_iter()
        .filter(|tool| tool.is_enabled)
        .collect();

    let llm_db = LLMDatabase::new(app_handle)?;
    let model_detail = llm_db
        .get_llm_model_detail(&provider_id, &model_code)
        .map_err(|e| AppError::DatabaseError(format!("获取 MCP 总结模型失败: {}", e)))?;

    let network_proxy = get_network_proxy_from_config(&config_map);
    let request_timeout = get_request_timeout_from_config(&config_map);
    let client = genai_client::create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        false,
        Some(request_timeout),
        &config_map,
    )?;

    let user_prompt = build_summary_user_prompt(&server, &tools);
    let message_list: Vec<(String, String, Vec<crate::db::conversation_db::MessageAttachment>)> = vec![
        ("system".to_string(), MCP_SUMMARIZER_SYSTEM_PROMPT.to_string(), Vec::new()),
        ("user".to_string(), user_prompt, Vec::new()),
    ];
    let chat_request = crate::api::ai::conversation::build_chat_request_from_messages(
        &message_list,
        crate::api::ai::conversation::ToolCallStrategy::NonNative,
        None,
    )
    .chat_request;

    let max_retry_attempts = get_retry_attempts_from_config(&config_map).max(1);
    let mut attempts = 0;
    let response_text = loop {
        match client.exec_chat(&model_detail.model.code, chat_request.clone(), None).await {
            Ok(response) => break response.first_text().unwrap_or("").to_string(),
            Err(e) => {
                attempts += 1;
                if attempts >= max_retry_attempts {
                    return Err(AppError::ProviderError(format!("MCP 摘要生成失败: {}", e)));
                }
                sleep(Duration::from_millis(calculate_retry_delay(attempts))).await;
            }
        }
    };

    let (server_summary, tool_summaries) = parse_summary_response(&response_text)
        .ok_or_else(|| AppError::ParseError("解析 MCP 摘要结果失败".to_string()))?;

    let mut updated_tools = 0usize;
    if let Some(server_summary) = server_summary {
        mcp_db.update_server_catalog_summary(server_id, &server_summary)?;
    }

    for tool in tools {
        let key = tool.tool_name.to_lowercase();
        if let Some(summary) = tool_summaries.get(&key) {
            mcp_db.update_tool_catalog_summary(tool.id, summary)?;
            updated_tools += 1;
        }
    }

    info!(server_id, updated_tools, "MCP catalog summaries updated");
    Ok(())
}
