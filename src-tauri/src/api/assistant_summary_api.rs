use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};
use tokio::time::{sleep, Duration};
use tracing::{debug, info, warn};

use crate::api::ai::config::{
    calculate_retry_delay, get_network_proxy_from_config, get_request_timeout_from_config,
    get_retry_attempts_from_config,
};
use crate::api::butler_api::{
    refresh_butler_system_assistant_if_ready, BUTLER_SYSTEM_ASSISTANT_NAME,
};
use crate::api::skill_api::get_enabled_assistant_skills_internal;
use crate::db::assistant_db::{Assistant, AssistantDatabase, AssistantSummary};
use crate::db::llm_db::LLMDatabase;
use crate::db::system_db::FeatureConfig;
use crate::errors::AppError;

const EXPERIMENTAL_FEATURE_CODE: &str = "experimental";
const ASSISTANT_SUMMARY_RETRY_PROMPT_LIMIT: usize = 3500;
static ASSISTANT_SUMMARY_RUNNING: AtomicBool = AtomicBool::new(false);
const ASSISTANT_SUMMARIZER_SYSTEM_PROMPT: &str = r#"你是 AIPP 的助手画像总结器。你的任务是把一个 AI 助手的名称、系统提示词、已启用 MCP 能力、已启用 Skills，压缩成总管家可读的执行画像。

输出要求：
1. 输出严格 JSON，不要附带解释。
2. JSON 格式固定为 {"summary":"...","tags":["..."]}。
3. `summary` 使用中文，不超过 100 个汉字，重点说明这个助手适合承接什么任务、依赖哪些能力、产出风格如何。
4. `tags` 返回 2-6 个短标签，适合用于路由和筛选；可以是中文或英文短词，但不要写句子。
5. 不要编造未给出的能力，不要把 MCP / Skills 原文大段复述。
6. 如果系统提示词很泛，但工具链很强，应优先总结其工具和技能侧优势。"#;

#[derive(Debug, Clone)]
struct AssistantSummaryModelSelection {
    model_code: String,
    provider_id: i64,
}

#[derive(Debug, Clone)]
struct AssistantToolSummaryInput {
    name: String,
    description: String,
    is_auto_run: bool,
}

#[derive(Debug, Clone)]
struct AssistantServerSummaryInput {
    name: String,
    tools: Vec<AssistantToolSummaryInput>,
}

#[derive(Debug, Clone)]
struct AssistantSkillSummaryInput {
    name: String,
    description: String,
    tags: Vec<String>,
}

#[derive(Debug, Clone)]
struct AssistantProfileInput {
    name: String,
    description: String,
    prompt: String,
    mcp_servers: Vec<AssistantServerSummaryInput>,
    skills: Vec<AssistantSkillSummaryInput>,
    source_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButlerAssistantDirectoryItem {
    pub assistant_id: i64,
    pub assistant_name: String,
    pub summary: String,
    pub tags: Vec<String>,
    pub mcp_server_names: Vec<String>,
    pub skill_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantSummaryProgressPayload {
    pub phase: String,
    pub total: usize,
    pub completed: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub assistant_name: Option<String>,
    pub message: Option<String>,
}

fn is_butler_reserved_assistant(assistant: &Assistant) -> bool {
    assistant.name.trim() == BUTLER_SYSTEM_ASSISTANT_NAME
}

pub fn is_assistant_summary_running() -> bool {
    ASSISTANT_SUMMARY_RUNNING.load(Ordering::SeqCst)
}

fn trim_chars(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn normalize_tag(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(|ch| ch == '"' || ch == '\'' || ch == '`');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn normalize_tags(tags: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();
    for tag in tags.into_iter().filter_map(|tag| normalize_tag(&tag)) {
        let dedupe_key = tag.to_lowercase();
        if seen.insert(dedupe_key) {
            normalized.push(tag);
        }
        if normalized.len() >= 6 {
            break;
        }
    }
    normalized
}

fn parse_tags_from_value(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => normalize_tags(
            items.iter().filter_map(|item| item.as_str().map(|text| text.to_string())).collect(),
        ),
        Some(Value::String(text)) => normalize_tags(
            text.split(|ch| matches!(ch, ',' | '，' | ';' | '；' | '\n' | '|'))
                .map(|item| item.to_string())
                .collect(),
        ),
        _ => Vec::new(),
    }
}

fn parse_summary_response(response_text: &str) -> Option<(String, Vec<String>)> {
    let json_value: Value = serde_json::from_str(response_text)
        .or_else(|_| {
            let start = response_text.find('{').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("missing summary json start"))
            })?;
            let end = response_text.rfind('}').ok_or_else(|| {
                serde_json::Error::io(std::io::Error::other("missing summary json end"))
            })?;
            serde_json::from_str(&response_text[start..=end])
        })
        .ok()?;

    let summary = json_value.get("summary")?.as_str()?.trim().to_string();
    if summary.is_empty() {
        return None;
    }
    let tags = parse_tags_from_value(json_value.get("tags"));
    Some((trim_chars(&summary, 100), tags))
}

fn build_summary_fallback(profile: &AssistantProfileInput) -> (String, Vec<String>) {
    let mut parts = Vec::new();
    let mut tags = Vec::new();

    if !profile.description.trim().is_empty() {
        parts.push(trim_chars(profile.description.trim(), 36));
    }

    if !profile.skills.is_empty() {
        let skill_names = profile
            .skills
            .iter()
            .map(|skill| skill.name.as_str())
            .take(3)
            .collect::<Vec<_>>()
            .join("、");
        parts.push(format!("可调用技能：{}", skill_names));
        tags.push("skills".to_string());
    }

    if !profile.mcp_servers.is_empty() {
        let server_names = profile
            .mcp_servers
            .iter()
            .map(|server| server.name.as_str())
            .take(3)
            .collect::<Vec<_>>()
            .join("、");
        parts.push(format!("可用MCP：{}", server_names));
        tags.push("mcp".to_string());
    }

    if parts.is_empty() && !profile.prompt.trim().is_empty() {
        parts.push(trim_chars(profile.prompt.trim(), 60));
    }

    if parts.is_empty() {
        parts.push("通用执行助手，可承接需要独立上下文的任务。".to_string());
    }

    for skill in profile.skills.iter().take(3) {
        tags.extend(skill.tags.iter().cloned());
    }
    if profile.mcp_servers.iter().any(|server| !server.tools.is_empty()) {
        tags.push("tool-use".to_string());
    }

    (trim_chars(&parts.join("；"), 100), normalize_tags(tags))
}

fn build_summary_user_prompt(
    profile: &AssistantProfileInput,
    prompt_limit: Option<usize>,
) -> String {
    let mcp_text = if profile.mcp_servers.is_empty() {
        "无".to_string()
    } else {
        profile
            .mcp_servers
            .iter()
            .map(|server| {
                let tools = if server.tools.is_empty() {
                    "无工具".to_string()
                } else {
                    server
                        .tools
                        .iter()
                        .map(|tool| {
                            let suffix = if tool.is_auto_run { " [auto]" } else { "" };
                            let desc = if tool.description.trim().is_empty() {
                                String::new()
                            } else {
                                format!(": {}", trim_chars(tool.description.trim(), 80))
                            };
                            format!("{}{}{}", tool.name, suffix, desc)
                        })
                        .collect::<Vec<_>>()
                        .join("; ")
                };
                format!("- {} => {}", server.name, tools)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let skill_text = if profile.skills.is_empty() {
        "无".to_string()
    } else {
        profile
            .skills
            .iter()
            .map(|skill| {
                let mut line = format!("- {}", skill.name);
                if !skill.description.trim().is_empty() {
                    line.push_str(&format!(": {}", trim_chars(skill.description.trim(), 80)));
                }
                if !skill.tags.is_empty() {
                    line.push_str(&format!(" [tags: {}]", skill.tags.join(", ")));
                }
                line
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let raw_prompt = profile.prompt.trim();
    let (prompt_label, prompt_text) = if raw_prompt.is_empty() {
        ("系统提示词", "无".to_string())
    } else if let Some(limit) = prompt_limit {
        if raw_prompt.chars().count() > limit {
            ("系统提示词（首次生成失败后，已截断重试）", trim_chars(raw_prompt, limit))
        } else {
            ("系统提示词", raw_prompt.to_string())
        }
    } else {
        ("系统提示词", raw_prompt.to_string())
    };

    format!(
        "助手名称：{name}\n助手描述：{description}\n\n{prompt_label}：\n{prompt}\n\n已启用 MCP：\n{mcp_text}\n\n已启用 Skills：\n{skill_text}",
        name = profile.name,
        description = if profile.description.trim().is_empty() {
            "无".to_string()
        } else {
            trim_chars(profile.description.trim(), 160)
        },
        prompt_label = prompt_label,
        prompt = prompt_text,
    )
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

fn parse_model_selection(
    config_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Option<AssistantSummaryModelSelection> {
    let experimental = config_map.get(EXPERIMENTAL_FEATURE_CODE)?;
    let raw_value = experimental.get("assistant_summarizer_model_id")?.value.trim().to_string();
    if raw_value.is_empty() {
        return None;
    }

    let parts: Vec<&str> = raw_value.split("%%").collect();
    if parts.len() != 2 {
        return None;
    }

    let model_code = parts[0].trim().to_string();
    let provider_id = parts[1].trim().parse::<i64>().ok()?;
    if model_code.is_empty() {
        return None;
    }

    Some(AssistantSummaryModelSelection { model_code, provider_id })
}

async fn collect_assistant_profile(
    app_handle: &AppHandle,
    assistant: &Assistant,
) -> Result<AssistantProfileInput, AppError> {
    let (prompt, mcp_servers) = {
        let assistant_db = AssistantDatabase::new(app_handle)?;
        let prompt = assistant_db
            .get_assistant_prompt(assistant.id)?
            .into_iter()
            .map(|prompt| prompt.prompt)
            .filter(|prompt| !prompt.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n\n");

        let mcp_servers = assistant_db
            .get_assistant_mcp_servers_with_tools(assistant.id)?
            .into_iter()
            .filter(|(_, _, _, server_enabled, _)| *server_enabled)
            .map(|(_, server_name, _, _, tools)| AssistantServerSummaryInput {
                name: server_name,
                tools: tools
                    .into_iter()
                    .filter(|(_, _, _, tool_enabled, _, _)| *tool_enabled)
                    .map(|(_, tool_name, tool_description, _, tool_is_auto_run, _)| {
                        AssistantToolSummaryInput {
                            name: tool_name,
                            description: tool_description,
                            is_auto_run: tool_is_auto_run,
                        }
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        (prompt, mcp_servers)
    };

    let skills = get_enabled_assistant_skills_internal(app_handle, assistant.id)
        .await?
        .into_iter()
        .map(|skill| AssistantSkillSummaryInput {
            name: skill.display_name,
            description: skill.metadata.description.unwrap_or_default(),
            tags: skill.metadata.tags,
        })
        .collect::<Vec<_>>();

    let source_payload = json!({
        "assistant_name": assistant.name,
        "description": assistant.description,
        "prompt": prompt,
        "mcp_servers": mcp_servers
            .iter()
            .map(|server| json!({
                "name": server.name,
                "tools": server.tools.iter().map(|tool| json!({
                    "name": tool.name,
                    "description": tool.description,
                    "is_auto_run": tool.is_auto_run,
                })).collect::<Vec<_>>(),
            }))
            .collect::<Vec<_>>(),
        "skills": skills
            .iter()
            .map(|skill| json!({
                "name": skill.name,
                "description": skill.description,
                "tags": skill.tags,
            }))
            .collect::<Vec<_>>(),
    });
    let source_hash = hex::encode(Sha256::digest(source_payload.to_string().as_bytes()));

    Ok(AssistantProfileInput {
        name: assistant.name.clone(),
        description: assistant.description.clone().unwrap_or_default(),
        prompt,
        mcp_servers,
        skills,
        source_hash,
    })
}

async fn generate_assistant_summary(
    app_handle: &AppHandle,
    assistant: &Assistant,
    model_selection: &AssistantSummaryModelSelection,
    config_map: &HashMap<String, HashMap<String, FeatureConfig>>,
) -> Result<(), AppError> {
    let profile = collect_assistant_profile(app_handle, assistant).await?;
    let llm_db = LLMDatabase::new(app_handle)?;
    let model_detail = llm_db
        .get_llm_model_detail(&model_selection.provider_id, &model_selection.model_code)
        .map_err(|e| AppError::DatabaseError(format!("获取助手总结模型失败: {}", e)))?;

    let network_proxy = get_network_proxy_from_config(config_map);
    let request_timeout = get_request_timeout_from_config(config_map);
    let client = crate::api::genai_client::create_client_with_config(
        &model_detail.configs,
        &model_detail.model.code,
        &model_detail.provider.api_type,
        network_proxy.as_deref(),
        false,
        Some(request_timeout),
        false,
        config_map,
    )?;

    let max_retry_attempts = get_retry_attempts_from_config(config_map).max(1);
    let mut attempts = 0;
    let response_text = loop {
        let prompt_limit =
            if attempts == 0 { None } else { Some(ASSISTANT_SUMMARY_RETRY_PROMPT_LIMIT) };
        let message_list = vec![
            ("system".to_string(), ASSISTANT_SUMMARIZER_SYSTEM_PROMPT.to_string(), Vec::new()),
            ("user".to_string(), build_summary_user_prompt(&profile, prompt_limit), Vec::new()),
        ];
        let chat_request = crate::api::ai::conversation::build_chat_request_from_messages(
            &message_list,
            crate::api::ai::conversation::ToolCallStrategy::NonNative,
            None,
        )
        .chat_request;

        match client.exec_chat(&model_detail.model.code, chat_request, None).await {
            Ok(response) => break response.first_text().unwrap_or("").to_string(),
            Err(error) => {
                attempts += 1;
                if attempts >= max_retry_attempts {
                    return Err(AppError::ProviderError(format!(
                        "助手摘要生成失败 (assistant_name={}): {}",
                        assistant.name, error
                    )));
                }
                warn!(
                    assistant_id = assistant.id,
                    assistant_name = %assistant.name,
                    retry_attempt = attempts,
                    prompt_limit = prompt_limit.unwrap_or(usize::MAX),
                    "assistant summary generation failed, retrying"
                );
                sleep(Duration::from_millis(calculate_retry_delay(attempts))).await;
            }
        }
    };

    let (summary, tags) = parse_summary_response(&response_text).ok_or_else(|| {
        warn!(
            assistant_id = assistant.id,
            assistant_name = %assistant.name,
            response = %response_text,
            "Failed to parse assistant summary response"
        );
        AppError::ParseError("解析助手摘要结果失败".to_string())
    })?;

    let assistant_db = AssistantDatabase::new(app_handle)?;
    assistant_db.upsert_assistant_summary(
        assistant.id,
        &summary,
        &serde_json::to_string(&tags).unwrap_or_else(|_| "[]".to_string()),
        &profile.source_hash,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        build_summary_user_prompt, AssistantProfileInput, AssistantServerSummaryInput,
        AssistantSkillSummaryInput, ASSISTANT_SUMMARY_RETRY_PROMPT_LIMIT,
    };

    fn build_profile(prompt: String) -> AssistantProfileInput {
        AssistantProfileInput {
            name: "测试助手".to_string(),
            description: "用于测试助手画像".to_string(),
            prompt,
            mcp_servers: vec![AssistantServerSummaryInput {
                name: "filesystem".to_string(),
                tools: Vec::new(),
            }],
            skills: vec![AssistantSkillSummaryInput {
                name: "spec-writing".to_string(),
                description: "撰写规格".to_string(),
                tags: vec!["docs".to_string()],
            }],
            source_hash: "test-hash".to_string(),
        }
    }

    #[test]
    fn build_summary_user_prompt_uses_full_prompt_on_first_attempt() {
        let prompt = "A".repeat(4200);
        let rendered = build_summary_user_prompt(&build_profile(prompt.clone()), None);

        assert!(rendered.contains("系统提示词："));
        assert!(!rendered.contains("已截断重试"));
        assert!(rendered.contains(&prompt));
    }

    #[test]
    fn build_summary_user_prompt_truncates_only_on_retry() {
        let prompt = "B".repeat(4200);
        let rendered = build_summary_user_prompt(
            &build_profile(prompt),
            Some(ASSISTANT_SUMMARY_RETRY_PROMPT_LIMIT),
        );

        assert!(rendered.contains("系统提示词（首次生成失败后，已截断重试）："));
        assert_eq!(rendered.matches('B').count(), ASSISTANT_SUMMARY_RETRY_PROMPT_LIMIT);
    }
}

fn emit_summary_progress(app_handle: &AppHandle, payload: AssistantSummaryProgressPayload) {
    let _ = app_handle.emit("assistant-summary-progress", payload);
}

fn parse_tags_json(tags_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(tags_json).map(normalize_tags).unwrap_or_default()
}

pub async fn list_assistant_directory_for_butler(
    app_handle: &AppHandle,
) -> Result<Vec<ButlerAssistantDirectoryItem>, String> {
    let mut assistants = {
        let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
        assistant_db
            .get_assistants()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|assistant| !is_butler_reserved_assistant(assistant))
            .collect::<Vec<_>>()
    };
    assistants.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

    let mut items = Vec::new();
    for assistant in assistants {
        let profile =
            collect_assistant_profile(app_handle, &assistant).await.map_err(|e| e.to_string())?;
        let stored_summary = {
            let assistant_db = AssistantDatabase::new(app_handle).map_err(|e| e.to_string())?;
            assistant_db.get_assistant_summary(assistant.id).map_err(|e| e.to_string())?
        };
        let (summary, tags) = match stored_summary {
            Some(AssistantSummary { summary, tags_json, source_hash, .. })
                if source_hash == profile.source_hash && !summary.trim().is_empty() =>
            {
                (trim_chars(summary.trim(), 100), parse_tags_json(&tags_json))
            }
            _ => build_summary_fallback(&profile),
        };

        items.push(ButlerAssistantDirectoryItem {
            assistant_id: assistant.id,
            assistant_name: assistant.name,
            summary,
            tags,
            mcp_server_names: profile.mcp_servers.into_iter().map(|server| server.name).collect(),
            skill_names: profile.skills.into_iter().map(|skill| skill.name).collect(),
        });
    }

    Ok(items)
}

pub async fn build_butler_assistant_directory_prompt(
    app_handle: &AppHandle,
) -> Result<String, String> {
    let items = list_assistant_directory_for_butler(app_handle).await?;
    if items.is_empty() {
        return Ok(
            "当前没有可派发的普通助手。若用户要求实质性工作，先提示其创建或配置至少一个普通助手。"
                .to_string(),
        );
    }

    let sections = items
        .into_iter()
        .map(|item| {
            let tags = if item.tags.is_empty() {
                "无".to_string()
            } else {
                item.tags.join(", ")
            };
            let mcp = if item.mcp_server_names.is_empty() {
                "无".to_string()
            } else {
                item.mcp_server_names.join(", ")
            };
            let skills = if item.skill_names.is_empty() {
                "无".to_string()
            } else {
                item.skill_names.join(", ")
            };
            format!(
                "- {name}（assistant_id={assistant_id}）\n  简介：{summary}\n  标签：{tags}\n  MCP：{mcp}\n  Skills：{skills}",
                name = item.assistant_name,
                assistant_id = item.assistant_id,
                summary = item.summary,
                tags = tags,
                mcp = mcp,
                skills = skills,
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "当前可派发助手目录：\n系统会先注入这份助手画像，再注入 MCP / Skills 运行时能力目录。派工前优先根据下列目录选择最合适的执行助手。\n{}",
        sections
    ))
}

#[tauri::command]
pub async fn summarize_all_assistant_summaries(app_handle: tauri::AppHandle) -> Result<(), String> {
    if ASSISTANT_SUMMARY_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("助手画像生成任务正在进行中，请稍后重试".to_string());
    }
    struct ResetRunning;
    impl Drop for ResetRunning {
        fn drop(&mut self) {
            ASSISTANT_SUMMARY_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _reset = ResetRunning;

    let config_map = get_feature_config_map(&app_handle).await.map_err(|e| e.to_string())?;
    let Some(model_selection) = parse_model_selection(&config_map) else {
        return Err("请先在实验性功能中选择助手总结 AI 模型".to_string());
    };

    let mut assistants = {
        let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        assistant_db
            .get_assistants()
            .map_err(|e| e.to_string())?
            .into_iter()
            .filter(|assistant| !is_butler_reserved_assistant(assistant))
            .collect::<Vec<_>>()
    };
    assistants.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));

    let total = assistants.len();
    emit_summary_progress(
        &app_handle,
        AssistantSummaryProgressPayload {
            phase: "started".to_string(),
            total,
            completed: 0,
            succeeded: 0,
            failed: 0,
            assistant_name: None,
            message: Some("开始生成助手画像".to_string()),
        },
    );

    let mut completed = 0usize;
    let mut succeeded = 0usize;
    let mut failed = 0usize;

    for assistant in assistants {
        emit_summary_progress(
            &app_handle,
            AssistantSummaryProgressPayload {
                phase: "processing".to_string(),
                total,
                completed,
                succeeded,
                failed,
                assistant_name: Some(assistant.name.clone()),
                message: Some("正在生成助手画像".to_string()),
            },
        );

        match generate_assistant_summary(&app_handle, &assistant, &model_selection, &config_map)
            .await
        {
            Ok(_) => {
                completed += 1;
                succeeded += 1;
                info!(assistant_id = assistant.id, assistant_name = %assistant.name, "assistant summary updated");
                emit_summary_progress(
                    &app_handle,
                    AssistantSummaryProgressPayload {
                        phase: "progress".to_string(),
                        total,
                        completed,
                        succeeded,
                        failed,
                        assistant_name: Some(assistant.name.clone()),
                        message: Some("助手画像已更新".to_string()),
                    },
                );
            }
            Err(error) => {
                completed += 1;
                failed += 1;
                warn!(assistant_id = assistant.id, assistant_name = %assistant.name, error = %error, "assistant summary failed");
                emit_summary_progress(
                    &app_handle,
                    AssistantSummaryProgressPayload {
                        phase: "progress".to_string(),
                        total,
                        completed,
                        succeeded,
                        failed,
                        assistant_name: Some(assistant.name.clone()),
                        message: Some(format!("生成失败: {}", error)),
                    },
                );
            }
        }
    }

    emit_summary_progress(
        &app_handle,
        AssistantSummaryProgressPayload {
            phase: "completed".to_string(),
            total,
            completed,
            succeeded,
            failed,
            assistant_name: None,
            message: Some("助手画像生成完成".to_string()),
        },
    );

    if let Err(error) = refresh_butler_system_assistant_if_ready(&app_handle).await {
        warn!(error = %error, "failed to refresh butler system assistant after assistant summary generation");
    }

    debug!(total, succeeded, failed, "assistant summary generation completed");
    Ok(())
}

pub async fn start_assistant_summary_generation(app_handle: tauri::AppHandle) -> Result<bool, String> {
    if is_assistant_summary_running() {
        return Ok(false);
    }

    let config_map = get_feature_config_map(&app_handle).await.map_err(|e| e.to_string())?;
    if parse_model_selection(&config_map).is_none() {
        return Err("请先在实验性功能中选择助手总结 AI 模型".to_string());
    }

    tauri::async_runtime::spawn(async move {
        if let Err(error) = summarize_all_assistant_summaries(app_handle).await {
            warn!(error = %error, "assistant summary background task failed");
        }
    });

    Ok(true)
}
