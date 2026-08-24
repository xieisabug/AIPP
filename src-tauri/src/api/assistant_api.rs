use crate::api::ai::acp::{
    apply_network_proxy_to_env_vars, build_acp_launch_plan, build_selected_mcp_tools_payload,
    extract_acp_config,
    refresh_acp_config_signature, refresh_acp_selected_mcp_tools_payload, resolve_acp_cli_path,
    spawn_acp_idle_reaper_once, spawn_acp_session_task, AcpSessionEntry,
};
use crate::api::ai::codex_app_server::{
    extract_codex_app_server_config, probe_codex_model_options, refresh_codex_session_signature, spawn_codex_session_task,
    CodexSessionEntry, CODEX_APP_SERVER_API_TYPE,
};
use crate::api::ai::claude_sdk::{
    claude_model_choices, extract_claude_sdk_config, is_claude_code_provider,
    CLAUDE_SDK_API_TYPE,
};
use crate::api::ai::config::get_network_proxy_from_config;
use crate::{
    api::butler_api::{is_butler_system_assistant, is_butler_system_assistant_name},
    db::{
        assistant_db::{
            Assistant, AssistantDatabase, AssistantMCPConfig, AssistantMCPToolConfig,
            AssistantModel, AssistantModelConfig, AssistantPrompt, AssistantPromptParam,
        },
        conversation_db::ConversationDatabase,
        llm_db::LLMDatabase,
    },
    utils::share_utils::{
        compress_assistant_data, decompress_assistant_data, AssistantShareData, ModelConfigShare,
        SharedAssistant,
    },
    FeatureConfigState, NameCacheState,
};
use std::collections::HashMap;
use tauri::{Emitter, Manager};
use tracing::{debug, info, instrument, warn};

#[derive(Debug, serde::Serialize, Clone)]
pub struct AgentModelOption {
    pub code: String,
    pub name: String,
    pub provider_id: i64,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
}

#[derive(Debug, serde::Serialize, Clone)]
pub struct AgentRuntimeInfo {
    pub agent_kind: String,
    pub working_directory: String,
}

pub const CLAUDE_CODE_DEFAULT_MODEL: &str = "__claude_code_default__";

fn add_agent_model_option(options: &mut Vec<AgentModelOption>, option: AgentModelOption) {
    if !options.iter().any(|existing| existing.code == option.code) {
        options.push(option);
    }
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub async fn get_agent_model_options(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<Vec<AgentModelOption>, String> {
    let detail = get_assistant(app_handle.clone(), assistant_id)?;
    if detail.assistant.assistant_type != Some(4) {
        return Ok(Vec::new());
    }
    let model = detail.model.first().ok_or("Agent 助手尚未配置提供商")?;
    let provider_id = model.provider_id;
    if provider_id <= 0 {
        return Err("Agent 助手尚未配置提供商".into());
    }
    let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let provider = llm_db.get_llm_provider(provider_id).map_err(|e| e.to_string())?;
    let provider_configs = llm_db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;
    let current_model = (!model.model_code.trim().is_empty()).then(|| model.model_code.trim().to_string());
    let db_models = llm_db.get_llm_models(provider_id.to_string()).map_err(|e| e.to_string())?;
    let mut options = Vec::new();

    match provider.api_type.as_str() {
        CODEX_APP_SERVER_API_TYPE => {
            let config = extract_codex_app_server_config(&detail.model_configs, &provider_configs, current_model.clone())
                .map_err(|e| e.to_string())?;
            let catalog = probe_codex_model_options(&app_handle, &config).await?;
            for entry in catalog {
                add_agent_model_option(&mut options, AgentModelOption {
                    code: format!("{}%%{}", entry.id, provider_id),
                    name: entry.name,
                    provider_id,
                    efforts: entry.supported_efforts,
                    default_effort: entry.default_effort,
                });
            }
        }
        api_type
            if api_type == CLAUDE_SDK_API_TYPE
                || (api_type == "acp"
                    && is_claude_code_provider(api_type, &provider_configs)) =>
        {
            let efforts = ["default", "low", "medium", "high", "max"].into_iter().map(str::to_string).collect::<Vec<_>>();
            for (code, name) in [
                ("sonnet", "Claude Sonnet"),
                ("opus", "Claude Opus"),
                ("haiku", "Claude Haiku"),
            ] {
                add_agent_model_option(&mut options, AgentModelOption { code: format!("{code}%%{provider_id}"), name: format!("Claude Code 默认配置 · {name}"), provider_id, efforts: efforts.clone(), default_effort: None });
            }
            add_agent_model_option(&mut options, AgentModelOption { code: format!("{CLAUDE_CODE_DEFAULT_MODEL}%%{provider_id}"), name: "Claude Code 默认配置 · 使用 CLI 默认模型".into(), provider_id, efforts: efforts.clone(), default_effort: None });
            for (other_id, other_name, other_api_type, _, _, _) in llm_db.get_llm_providers().map_err(|e| e.to_string())? {
                if other_api_type != "anthropic" { continue; }
                for (_, model_name, _, code, _, _, _, _) in llm_db.get_llm_models(other_id.to_string()).map_err(|e| e.to_string())? {
                    add_agent_model_option(&mut options, AgentModelOption { code: format!("{code}%%{other_id}"), name: format!("{other_name} / {model_name}"), provider_id: other_id, efforts: efforts.clone(), default_effort: None });
                }
            }
            if let Some(code) = current_model {
                add_agent_model_option(&mut options, AgentModelOption { code: format!("{code}%%{provider_id}"), name: format!("Claude Code 默认配置 · 当前模型 {code}"), provider_id, efforts, default_effort: None });
            }
        }
        "anthropic" => {
            let efforts = ["default", "low", "medium", "high", "max"]
                .into_iter()
                .map(str::to_string)
                .collect::<Vec<_>>();
            for (_, name, _, code, _, _, _, _) in db_models {
                add_agent_model_option(&mut options, AgentModelOption {
                    code: format!("{code}%%{provider_id}"),
                    name,
                    provider_id,
                    efforts: efforts.clone(),
                    default_effort: None,
                });
            }
            if let Some(code) = current_model {
                add_agent_model_option(&mut options, AgentModelOption {
                    code: format!("{code}%%{provider_id}"),
                    name: code,
                    provider_id,
                    efforts,
                    default_effort: None,
                });
            }
        }
        "acp" => {
            for (_, name, _, code, _, _, _, _) in db_models {
                add_agent_model_option(&mut options, AgentModelOption { code: format!("{code}%%{provider_id}"), name, provider_id, efforts: Vec::new(), default_effort: None });
            }
            if let Some(code) = current_model {
                add_agent_model_option(&mut options, AgentModelOption { code: format!("{code}%%{provider_id}"), name: code.clone(), provider_id, efforts: Vec::new(), default_effort: None });
            }
        }
        other => return Err(format!("不支持的 Agent 提供商类型：{other}")),
    }
    if options.is_empty() {
        return Err(format!("提供商 {} 没有可用模型", provider.name));
    }
    Ok(options)
}

fn reject_reserved_butler_assistant_name(name: &str) -> Result<(), String> {
    if is_butler_system_assistant_name(name) {
        Err("该助手名称为系统保留名称，不能创建或修改".to_string())
    } else {
        Ok(())
    }
}

pub(crate) fn resolve_acp_provider_id(
    models: &[AssistantModel],
    model_configs: &[AssistantModelConfig],
) -> Option<i64> {
    models
        .first()
        .map(|model| model.provider_id)
        .filter(|provider_id| *provider_id > 0)
        .or_else(|| {
            model_configs
                .iter()
                .find(|config| config.name == "acp_provider")
                .and_then(|config| config.value.as_deref())
                .and_then(|value| value.trim().parse::<i64>().ok())
                .filter(|provider_id| *provider_id > 0)
        })
}

fn get_or_insert_codex_auto_connect_session<T, F>(
    sessions: &mut HashMap<i64, T>,
    conversation_id: i64,
    create_session: F,
) -> &T
where
    F: FnOnce() -> T,
{
    sessions.entry(conversation_id).or_insert_with(create_session)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assistant_model(provider_id: i64) -> AssistantModel {
        AssistantModel {
            id: 1,
            assistant_id: 1,
            provider_id,
            model_code: String::new(),
            alias: String::new(),
        }
    }

    fn model_config(name: &str, value: Option<&str>) -> AssistantModelConfig {
        AssistantModelConfig {
            id: 1,
            assistant_id: 1,
            assistant_model_id: 1,
            name: name.to_string(),
            value: value.map(str::to_string),
            value_type: "string".to_string(),
        }
    }

    #[test]
    fn resolve_acp_provider_id_prefers_model_provider_id() {
        let models = vec![assistant_model(7)];
        let model_configs = vec![model_config("acp_provider", Some("9"))];

        assert_eq!(resolve_acp_provider_id(&models, &model_configs), Some(7));
    }

    #[test]
    fn resolve_acp_provider_id_uses_legacy_model_config_when_model_provider_is_empty() {
        let models = vec![assistant_model(0)];
        let model_configs = vec![model_config("acp_provider", Some("9"))];

        assert_eq!(resolve_acp_provider_id(&models, &model_configs), Some(9));
    }

    #[test]
    fn codex_auto_connect_never_replaces_ask_ai_session_with_different_signature() {
        #[derive(Debug, PartialEq)]
        struct SessionIdentity {
            run_id: &'static str,
            signature: &'static str,
        }

        let conversation_id = 792;
        let mut sessions = HashMap::from([(
            conversation_id,
            SessionIdentity {
                run_id: "ask-ai-run",
                signature: "message-model-override",
            },
        )]);
        let mut auto_connect_created_session = false;

        let selected = get_or_insert_codex_auto_connect_session(
            &mut sessions,
            conversation_id,
            || {
                auto_connect_created_session = true;
                SessionIdentity {
                    run_id: "auto-connect-run",
                    signature: "assistant-default-model",
                }
            },
        );

        assert_eq!(selected.run_id, "ask-ai-run");
        assert_eq!(selected.signature, "message-model-override");
        assert!(!auto_connect_created_session);
        assert_eq!(sessions[&conversation_id].run_id, "ask-ai-run");
    }
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AssistantDetail {
    pub assistant: Assistant,
    pub prompts: Vec<AssistantPrompt>,
    pub model: Vec<AssistantModel>,
    pub model_configs: Vec<AssistantModelConfig>,
    pub prompt_params: Vec<AssistantPromptParam>,
    pub mcp_configs: Vec<AssistantMCPConfig>,
    pub mcp_tool_configs: Vec<AssistantMCPToolConfig>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct MCPServerInfo {
    pub id: i64,
    pub name: String,
    pub is_enabled: bool,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct MCPToolInfo {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub is_enabled: bool,
    pub is_auto_run: bool,
    pub parameters: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct MCPServerWithTools {
    pub id: i64,
    pub name: String,
    pub summary: String,
    pub command: Option<String>,
    pub is_enabled: bool,
    pub tools: Vec<MCPToolInfo>,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AcpProxyEnvEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AcpCommandProbe {
    pub success: bool,
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct AcpLaunchDiagnostics {
    pub assistant_id: i64,
    pub provider_id: i64,
    pub cli_command: String,
    pub resolved_cli_path: String,
    pub working_directory: String,
    pub additional_args: Vec<String>,
    pub effective_program: String,
    pub effective_args: Vec<String>,
    pub proxy_strategy: String,
    pub proxy_enabled: bool,
    pub network_proxy: Option<String>,
    pub injected_proxy_env_count: usize,
    pub explicit_proxy_env_keys: Vec<String>,
    pub proxy_env: Vec<AcpProxyEnvEntry>,
    pub all_env_keys: Vec<String>,
    pub version_probe: AcpCommandProbe,
    pub notes: Vec<String>,
}

fn is_proxy_env_key(key: &str) -> bool {
    matches!(key.to_ascii_lowercase().as_str(), "http_proxy" | "https_proxy" | "all_proxy")
}

fn trim_probe_output(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value).trim().to_string();
    const LIMIT: usize = 1000;
    if text.len() <= LIMIT {
        text
    } else {
        format!("{}...", &text[..LIMIT])
    }
}

#[tauri::command]
#[instrument(skip(app_handle))]
pub fn get_assistants(app_handle: tauri::AppHandle) -> Result<Vec<Assistant>, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    debug!("loading assistants from database");
    assistant_db
        .get_assistants()
        .map(|assistants| {
            assistants
                .into_iter()
                .filter(|assistant| !is_butler_system_assistant(assistant))
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub fn get_assistant(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<AssistantDetail, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // 获取 Assistant 基本信息
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    info!(name = ?assistant.name, id = assistant.id, "loaded assistant");

    // 获取相关的 prompt
    let prompts = assistant_db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;
    debug!(count = prompts.len(), "assistant prompts loaded");

    // 获取相关的 model
    let model = assistant_db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
    debug!(model_count = model.len(), "assistant models loaded");

    // 获取相关的 model_config
    let model_configs =
        assistant_db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;
    debug!(model_config_count = model_configs.len(), "assistant model configs loaded");

    // 获取相关的 prompt_params
    let prompt_params =
        assistant_db.get_assistant_prompt_params(assistant_id).map_err(|e| e.to_string())?;
    debug!(prompt_param_count = prompt_params.len(), "assistant prompt params loaded");

    // 获取相关的 MCP 配置
    let mcp_configs =
        assistant_db.get_assistant_mcp_configs(assistant_id).map_err(|e| e.to_string())?;
    debug!(mcp_config_count = mcp_configs.len(), "assistant mcp configs loaded");

    // 获取相关的 MCP 工具配置
    let mcp_tool_configs =
        assistant_db.get_assistant_mcp_tool_configs(assistant_id).map_err(|e| e.to_string())?;
    debug!(mcp_tool_config_count = mcp_tool_configs.len(), "assistant mcp tool configs loaded");

    // 构建 AssistantDetail 对象
    let assistant_detail = AssistantDetail {
        assistant,
        prompts,
        model,
        model_configs,
        prompt_params,
        mcp_configs,
        mcp_tool_configs,
    };

    Ok(assistant_detail)
}

#[tauri::command]
#[instrument(skip(app_handle, name_cache_state, assistant_detail), fields(assistant_id = assistant_detail.assistant.id))]
pub async fn save_assistant(
    app_handle: tauri::AppHandle,
    name_cache_state: tauri::State<'_, NameCacheState>,
    assistant_detail: AssistantDetail,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    info!("save assistant start");
    debug!(?assistant_detail, "assistant detail incoming");
    reject_reserved_butler_assistant_name(&assistant_detail.assistant.name)?;
    let assistant_id = assistant_detail.assistant.id;
    let selected_agent_provider_id = if assistant_detail.assistant.assistant_type == Some(4) {
        let provider_id = assistant_detail
            .model
            .first()
            .map(|model| model.provider_id)
            .filter(|provider_id| *provider_id > 0)
            .ok_or_else(|| "Agent 助手尚未选择提供商，请先选择后再保存".to_string())?;
        let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        llm_db.get_llm_provider(provider_id).map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => format!(
                "Agent 助手引用的提供商 ID {provider_id} 不存在，请重新选择提供商后再保存"
            ),
            other => other.to_string(),
        })?;
        Some(provider_id)
    } else {
        None
    };
    if assistant_detail.assistant.id != 0 {
        let existing_assistant =
            assistant_db.get_assistant(assistant_detail.assistant.id).map_err(|e| e.to_string())?;
        if is_butler_system_assistant(&existing_assistant) {
            return Err("系统保留的总管家助手不能修改".to_string());
        }
    }

    // Save or update the Assistant
    if assistant_detail.assistant.id == 0 {
        assistant_db
            .add_assistant(
                &assistant_detail.assistant.name,
                assistant_detail.assistant.description.as_deref().unwrap_or(""),
                assistant_detail.assistant.assistant_type,
                true,
            )
            .map_err(|e| e.to_string())?;
    } else {
        assistant_db
            .update_assistant(
                assistant_detail.assistant.id,
                &assistant_detail.assistant.name,
                assistant_detail.assistant.description.as_deref().unwrap_or(""),
            )
            .map_err(|e| e.to_string())?;
    }

    // Update the name_cache_state
    let mut model_names = name_cache_state.assistant_names.lock().await;
    model_names.insert(assistant_detail.assistant.id, assistant_detail.assistant.name);

    // Save or update the AssistantPrompts
    for prompt in assistant_detail.prompts {
        if prompt.id == 0 {
            assistant_db
                .add_assistant_prompt(prompt.assistant_id, &prompt.prompt)
                .map_err(|e| e.to_string())?;
        } else {
            assistant_db
                .update_assistant_prompt(prompt.id, &prompt.prompt)
                .map_err(|e| e.to_string())?;
        }
    }

    // Save or update the AssistantModels
    for mut model in assistant_detail.model {
        if model.id == 0 {
            let result_id = assistant_db
                .add_assistant_model(
                    model.assistant_id,
                    model.provider_id,
                    &model.model_code,
                    &model.alias,
                )
                .map_err(|e| e.to_string())?;
            model.id = result_id;
        } else {
            assistant_db
                .update_assistant_model(
                    model.id,
                    model.provider_id,
                    &model.model_code,
                    &model.alias,
                )
                .map_err(|e| e.to_string())?;
        }
    }

    // 旧版本把 Agent provider 同时存进 acp_provider。若该字段仍存在，
    // 保存时同步为权威的 assistant_model.provider_id，避免旧值再次污染表单。
    if let Some(provider_id) = selected_agent_provider_id {
        if let Some(legacy_config) = assistant_db
            .get_assistant_model_configs(assistant_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|config| config.name == "acp_provider")
        {
            assistant_db
                .update_assistant_model_config(
                    legacy_config.id,
                    "acp_provider",
                    &provider_id.to_string(),
                )
                .map_err(|e| e.to_string())?;
        }
    }

    // Save or update the AssistantModelConfigs
    for mut config in assistant_detail.model_configs {
        if config.id == 0 {
            let result_id = assistant_db
                .add_assistant_model_config(
                    config.assistant_id,
                    config.assistant_model_id,
                    &config.name,
                    config.value.as_deref().unwrap_or(""),
                    &config.value_type,
                )
                .map_err(|e| e.to_string())?;
            config.id = result_id;
        } else {
            assistant_db
                .update_assistant_model_config(
                    config.id,
                    &config.name,
                    config.value.as_deref().unwrap_or(""),
                )
                .map_err(|e| e.to_string())?;
        }
    }

    // Save or update the AssistantPromptParams
    for param in assistant_detail.prompt_params {
        if param.id == 0 {
            assistant_db
                .add_assistant_prompt_param(
                    param.assistant_id,
                    param.assistant_prompt_id,
                    &param.param_name,
                    param.param_type.as_deref().unwrap_or(""),
                    param.param_value.as_deref().unwrap_or(""),
                )
                .map_err(|e| e.to_string())?;
        } else {
            assistant_db
                .update_assistant_prompt_param(
                    param.id,
                    &param.param_name,
                    param.param_type.as_deref().unwrap_or(""),
                    param.param_value.as_deref().unwrap_or(""),
                )
                .map_err(|e| e.to_string())?;
        }
    }

    // 广播助手列表更新事件
    let _ = app_handle.emit("assistant_list_changed", ());
    crate::sync::schedule_sync_after_local_change(&app_handle);

    Ok(())
}

#[tauri::command]
#[instrument(skip(app_handle, name, description), fields(assistant_type))]
pub fn add_assistant(
    app_handle: tauri::AppHandle,
    name: String,
    description: String,
    assistant_type: i64,
) -> Result<AssistantDetail, String> {
    info!("add assistant start");
    reject_reserved_butler_assistant_name(&name)?;
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Add a default assistant
    let assistant_id = assistant_db
        .add_assistant(&name, &description, Some(assistant_type), false)
        .map_err(|e| e.to_string())?;

    // Get the newly added assistant
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    info!(id = assistant_id, name = ?assistant.name, "assistant created");

    let default_prompt = "You are a helpful assistant.";
    let prompt_id = assistant_db
        .add_assistant_prompt(assistant_id, default_prompt)
        .map_err(|e| e.to_string())?;
    let prompts = vec![AssistantPrompt {
        id: prompt_id,
        assistant_id: assistant_id,
        prompt: default_prompt.to_string(),
        created_time: Option::None,
    }];

    let model_id =
        assistant_db.add_assistant_model(assistant_id, 0, "", "").map_err(|e| e.to_string())?;
    debug!(model_id, "added default model");

    // Add default model configs
    let default_model_configs = vec![
        AssistantModelConfig {
            id: 0,
            assistant_id,
            assistant_model_id: model_id, // Assuming 0 is a default model ID
            name: "max_tokens".to_string(),
            value: Some("2000".to_string()),
            value_type: "number".to_string(),
        },
        AssistantModelConfig {
            id: 0,
            assistant_id,
            assistant_model_id: model_id, // Assuming 0 is a default model ID
            name: "temperature".to_string(),
            value: Some("0.7".to_string()),
            value_type: "float".to_string(),
        },
        AssistantModelConfig {
            id: 0,
            assistant_id,
            assistant_model_id: model_id, // Assuming 0 is a default model ID
            name: "top_p".to_string(),
            value: Some("1.0".to_string()),
            value_type: "float".to_string(),
        },
        AssistantModelConfig {
            id: 0,
            assistant_id,
            assistant_model_id: model_id, // Assuming 0 is a default model ID
            name: "stream".to_string(),
            value: Some("true".to_string()),
            value_type: "boolean".to_string(),
        },
    ];
    let mut model_configs = Vec::new();
    for config in default_model_configs {
        let config_id = assistant_db
            .add_assistant_model_config(
                config.assistant_id,
                config.assistant_model_id,
                &config.name,
                config.value.as_deref().unwrap_or(""),
                &config.value_type,
            )
            .map_err(|e| e.to_string())?;
        model_configs.push(AssistantModelConfig {
            id: config_id,
            assistant_id: config.assistant_id,
            assistant_model_id: config.assistant_model_id,
            name: config.name,
            value: config.value,
            value_type: config.value_type,
        });
    }
    debug!(model_config_count = model_configs.len(), "default model configs inserted");

    // Model and prompt params are empty
    let model = vec![AssistantModel {
        id: model_id,
        assistant_id,
        provider_id: 0,
        model_code: "".to_string(),
        alias: "".to_string(),
    }];
    let prompt_params = Vec::new();

    // Build AssistantDetail object
    let assistant_detail = AssistantDetail {
        assistant,
        prompts,
        model,
        model_configs,
        prompt_params,
        mcp_configs: Vec::new(),
        mcp_tool_configs: Vec::new(),
    };

    // 广播助手列表更新事件
    let _ = app_handle.emit("assistant_list_changed", ());
    crate::sync::schedule_sync_after_local_change(&app_handle);

    Ok(assistant_detail)
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub fn copy_assistant(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<AssistantDetail, String> {
    info!("copy assistant start");
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Get the original assistant
    let original_assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    if is_butler_system_assistant(&original_assistant) {
        return Err("系统保留的总管家助手不能复制".to_string());
    }

    // Create a new assistant based on the original
    let new_assistant_id = assistant_db
        .add_assistant(
            &format!("副本 {}", original_assistant.name),
            &original_assistant.description.unwrap(),
            original_assistant.assistant_type,
            original_assistant.is_addition,
        )
        .map_err(|e| e.to_string())?;

    // Copy prompts
    let original_prompts =
        assistant_db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;
    let mut new_prompts = Vec::new();
    for prompt in original_prompts {
        let new_prompt_id = assistant_db
            .add_assistant_prompt(new_assistant_id, &prompt.prompt)
            .map_err(|e| e.to_string())?;
        new_prompts.push(AssistantPrompt {
            id: new_prompt_id,
            assistant_id: new_assistant_id,
            prompt: prompt.prompt,
            created_time: None,
        });
    }

    // Copy models and their configs
    let original_models =
        assistant_db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
    let mut new_models = Vec::new();
    let mut new_model_configs = Vec::new();
    for model in original_models {
        let new_model_id = assistant_db
            .add_assistant_model(
                new_assistant_id,
                model.provider_id,
                &model.model_code,
                &model.alias,
            )
            .map_err(|e| e.to_string())?;
        new_models.push(AssistantModel {
            id: new_model_id,
            assistant_id: new_assistant_id,
            provider_id: model.provider_id,
            model_code: model.model_code,
            alias: model.alias,
        });

        // Copy model configs
        let original_configs = assistant_db
            .get_assistant_model_configs_with_model_id(assistant_id, model.id)
            .map_err(|e| e.to_string())?;
        for config in original_configs {
            let new_config_id = assistant_db
                .add_assistant_model_config(
                    new_assistant_id,
                    new_model_id,
                    &config.name,
                    config.value.as_deref().unwrap_or(""),
                    &config.value_type,
                )
                .map_err(|e| e.to_string())?;
            new_model_configs.push(AssistantModelConfig {
                id: new_config_id,
                assistant_id: new_assistant_id,
                assistant_model_id: new_model_id,
                name: config.name,
                value: config.value,
                value_type: config.value_type,
            });
        }
    }

    // Get the newly created assistant
    let new_assistant = assistant_db.get_assistant(new_assistant_id).map_err(|e| e.to_string())?;

    // Build AssistantDetail object
    let assistant_detail = AssistantDetail {
        assistant: new_assistant,
        prompts: new_prompts,
        model: new_models,
        model_configs: new_model_configs,
        prompt_params: Vec::new(), // Assuming prompt_params are not copied
        mcp_configs: Vec::new(),
        mcp_tool_configs: Vec::new(),
    };

    info!(new_assistant_id, "assistant copied");

    // 广播助手列表更新事件
    let _ = app_handle.emit("assistant_list_changed", ());
    crate::sync::schedule_sync_after_local_change(&app_handle);

    Ok(assistant_detail)
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub fn delete_assistant(app_handle: tauri::AppHandle, assistant_id: i64) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    // 需要检查一下是不是快速使用助手，如果是，就不能够删除
    if assistant_id == 1 {
        return Err("快速使用助手不能删除".to_string());
    }
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    if is_butler_system_assistant(&assistant) {
        return Err("系统保留的总管家助手不能删除".to_string());
    }

    let _ = assistant_db
        .delete_assistant_model_config_by_assistant_id(assistant_id)
        .map_err(|e| e.to_string());
    let _ = assistant_db
        .delete_assistant_prompt_by_assistant_id(assistant_id)
        .map_err(|e| e.to_string());
    let _ = assistant_db
        .delete_assistant_prompt_param_by_assistant_id(assistant_id)
        .map_err(|e| e.to_string());

    let conversation_db = ConversationDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let _ = conversation_db
        .conversation_repo()
        .unwrap()
        .update_assistant_id(assistant_id, Some(1))
        .map_err(|e| e.to_string())?;

    assistant_db.delete_assistant(assistant_id).map_err(|e| e.to_string())?;

    // 广播助手列表更新事件
    let _ = app_handle.emit("assistant_list_changed", ());
    crate::sync::schedule_sync_after_local_change(&app_handle);

    Ok(())
}

#[tauri::command]
#[instrument(skip(app_handle, field_name), fields(assistant_id, field = field_name))]
pub fn get_assistant_field_value(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    field_name: &str,
) -> Result<String, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    if field_name == "prompt" {
        // Get prompts for this assistant
        let prompts = assistant_db.get_assistant_prompt(assistant_id).map_err(|e| e.to_string())?;

        debug!(prompt_count = prompts.len(), "prompts fetched for assistant");

        // Return first prompt's content
        return prompts
            .first()
            .map(|p| p.prompt.clone())
            .ok_or_else(|| "No prompt found".to_string());
    }

    // Get all model configs for this assistant
    let configs =
        assistant_db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;

    debug!(config_count = configs.len(), "model configs fetched for assistant");

    // Find config with matching name
    configs
        .iter()
        .find(|config| config.name == field_name)
        .and_then(|config| config.value.clone())
        .ok_or_else(|| format!("Field '{}' not found", field_name))
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub fn get_acp_working_directory(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<String, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;

    if assistant.assistant_type != Some(4) {
        return Err("Assistant is not ACP type".to_string());
    }

    let model_configs =
        assistant_db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;
    let assistant_models =
        assistant_db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
    let (provider_api_type, provider_configs) = if let Some(provider_id) =
        resolve_acp_provider_id(&assistant_models, &model_configs)
    {
        let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let provider = llm_db.get_llm_provider(provider_id).map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => format!(
                "Agent 助手 {assistant_id} 引用的提供商 ID {provider_id} 不存在，请在助手配置中重新选择提供商"
            ),
            other => other.to_string(),
        })?;
        let configs = llm_db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;
        (provider.api_type, configs)
    } else {
        return Err("Agent 助手尚未配置模型提供商".to_string());
    };

    // Codex app-server 通道：使用 Codex 配置提取，不能走 ACP 的 acp_cli_command 配置
    if provider_api_type == CODEX_APP_SERVER_API_TYPE {
        let model_code = assistant_models
            .first()
            .map(|model| model.model_code.clone())
            .filter(|value| !value.trim().is_empty());
        let codex_config =
            extract_codex_app_server_config(&model_configs, &provider_configs, model_code)
                .map_err(|e| e.to_string())?;
        return Ok(codex_config.working_directory.display().to_string());
    }

    if is_claude_code_provider(&provider_api_type, &provider_configs) {
        let model_code = assistant_models
            .first()
            .map(|model| model.model_code.clone())
            .filter(|value| !value.trim().is_empty());
        let model_provider_id = resolve_acp_provider_id(&assistant_models, &model_configs)
            .ok_or_else(|| "Claude Code 助手没有配置模型提供商".to_string())?;
        let model_choices = claude_model_choices(
            &LLMDatabase::new(&app_handle)
                .map_err(|e| e.to_string())?
                .get_llm_models(model_provider_id.to_string())
                .map_err(|e| e.to_string())?,
        );
        let claude_config = extract_claude_sdk_config(&model_configs, &provider_configs, model_code, model_choices)
            .map_err(|e| e.to_string())?;
        return Ok(claude_config.working_directory.display().to_string());
    }

    let acp_config = crate::api::ai::acp::extract_acp_config(&model_configs, &provider_configs)
        .map_err(|e| e.to_string())?;

    Ok(acp_config.working_directory.display().to_string())
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub fn get_agent_runtime_info(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<AgentRuntimeInfo, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;
    if assistant.assistant_type != Some(4) {
        return Err("Assistant is not Agent type".to_string());
    }
    let model_configs = assistant_db
        .get_assistant_model_configs(assistant_id)
        .map_err(|e| e.to_string())?;
    let assistant_models = assistant_db
        .get_assistant_model(assistant_id)
        .map_err(|e| e.to_string())?;
    let provider_id = resolve_acp_provider_id(&assistant_models, &model_configs)
        .ok_or_else(|| "Agent 助手尚未配置提供商".to_string())?;
    let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let provider = llm_db.get_llm_provider(provider_id).map_err(|error| match error {
        rusqlite::Error::QueryReturnedNoRows => format!(
            "Agent 助手 {assistant_id} 引用的提供商 ID {provider_id} 不存在，请在助手配置中重新选择提供商"
        ),
        other => other.to_string(),
    })?;
    let provider_configs = llm_db
        .get_llm_provider_config(provider_id)
        .map_err(|e| e.to_string())?;
    let agent_kind = if is_claude_code_provider(&provider.api_type, &provider_configs) {
        CLAUDE_SDK_API_TYPE.to_string()
    } else {
        provider.api_type
    };
    let working_directory = get_acp_working_directory(app_handle, assistant_id)?;
    Ok(AgentRuntimeInfo {
        agent_kind,
        working_directory,
    })
}

#[tauri::command]
#[instrument(skip(app_handle, window, acp_session_state, codex_session_state, claude_session_state), fields(conversation_id, assistant_id))]
pub async fn ensure_acp_session_connected(
    window: tauri::Window,
    app_handle: tauri::AppHandle,
    acp_session_state: tauri::State<'_, crate::AcpSessionState>,
    codex_session_state: tauri::State<'_, crate::CodexSessionState>,
    claude_session_state: tauri::State<'_, crate::ClaudeSessionState>,
    conversation_id: i64,
    assistant_id: i64,
) -> Result<Option<serde_json::Value>, String> {
    spawn_acp_idle_reaper_once(app_handle.clone());

    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;

    if assistant.assistant_type != Some(4) {
        return Ok(None);
    }

    // ask_ai may have just created the authoritative Claude session using a
    // per-message provider/model override. Never auto-connect a second session
    // for the same conversation and replace its state.
    if let Some(entry) = claude_session_state.sessions.lock().await.get(&conversation_id) {
        return Ok(Some(serde_json::to_value(entry.snapshot.clone()).unwrap_or(serde_json::Value::Null)));
    }

    let model_configs =
        assistant_db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;
    let assistant_models =
        assistant_db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
    let (provider_api_type, provider_configs) = if let Some(provider_id) =
        resolve_acp_provider_id(&assistant_models, &model_configs)
    {
        let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
        let provider = llm_db.get_llm_provider(provider_id).map_err(|error| match error {
            rusqlite::Error::QueryReturnedNoRows => format!(
                "Agent 助手 {assistant_id} 引用的提供商 ID {provider_id} 不存在，请在助手配置中重新选择提供商"
            ),
            other => other.to_string(),
        })?;
        let configs = llm_db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;
        (provider.api_type, configs)
    } else {
        return Err("Agent 助手尚未配置模型提供商".to_string());
    };

    // Claude Code's stream-json process is demand-driven by ask_ai because the
    // selected provider/model can be overridden per message. Auto-connect must
    // not start a default process that can race with and replace that session.
    if is_claude_code_provider(&provider_api_type, &provider_configs) {
        return Ok(None);
    }

    // 自动连接只负责填补空会话。ask_ai 可能已按本次消息的模型覆盖创建了权威会话，
    // 此处绝不能用助手默认配置替换它，否则会中断正在执行的 Codex turn。
    if provider_api_type == CODEX_APP_SERVER_API_TYPE {
        let model_code = assistant_models
            .first()
            .map(|model| model.model_code.clone())
            .filter(|value| !value.trim().is_empty());
        let mut codex_config =
            extract_codex_app_server_config(&model_configs, &provider_configs, model_code)
                .map_err(|e| e.to_string())?;
        codex_config.selected_mcp_tools_payload =
            build_selected_mcp_tools_payload(&app_handle, assistant_id)?;
        refresh_codex_session_signature(&mut codex_config);
        let snapshot = {
            let mut sessions = codex_session_state.sessions.lock().await;
            Some(
                get_or_insert_codex_auto_connect_session(
                    &mut sessions,
                    conversation_id,
                    || {
                        let handle = spawn_codex_session_task(
                            app_handle.clone(),
                            conversation_id,
                            codex_config.clone(),
                        );
                        CodexSessionEntry::new(
                            handle,
                            conversation_id,
                            codex_config.session_signature.clone(),
                        )
                    },
                )
                .snapshot
                .clone(),
            )
        };
        return Ok(snapshot
            .map(|state| serde_json::to_value(state).unwrap_or(serde_json::Value::Null)));
    }

    let proxy_enabled = provider_configs
        .iter()
        .find(|config| config.name == "proxy_enabled")
        .and_then(|config| config.value.parse::<bool>().ok())
        .unwrap_or(false);

    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    let network_proxy =
        proxy_enabled.then(|| get_network_proxy_from_config(&config_feature_map)).flatten();

    let mut acp_config = extract_acp_config(&model_configs, &provider_configs)
        .map_err(|e| e.to_string())?;
    if let Some(proxy_url) = network_proxy.as_deref() {
        apply_network_proxy_to_env_vars(&mut acp_config.env_vars, proxy_url);
    }
    refresh_acp_selected_mcp_tools_payload(&app_handle, assistant_id, &mut acp_config)?;
    refresh_acp_config_signature(&mut acp_config);

    let handle = {
        let mut sessions = acp_session_state.sessions.lock().await;
        if sessions
            .get(&conversation_id)
            .is_some_and(|entry| entry.config_signature == acp_config.session_signature)
        {
            let entry = sessions.get_mut(&conversation_id).expect("checked above");
            entry.touch();
            entry.handle.clone()
        } else {
            if sessions.contains_key(&conversation_id) {
                info!(
                    conversation_id,
                    "ACP session config changed during auto-connect; replacing existing session"
                );
            }
            let handle = spawn_acp_session_task(
                app_handle.clone(),
                conversation_id,
                acp_config.clone(),
            );
            sessions.insert(
                conversation_id,
                AcpSessionEntry::new(
                    handle.clone(),
                    conversation_id,
                    acp_config.session_signature.clone(),
                ),
            );
            handle
        }
    };

    handle.start(window).await.map_err(|error| error.to_string())?;

    let sessions = acp_session_state.sessions.lock().await;
    Ok(sessions
        .get(&conversation_id)
        .map(|entry| serde_json::to_value(&entry.snapshot).unwrap_or(serde_json::Value::Null)))
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub async fn get_acp_launch_diagnostics(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<AcpLaunchDiagnostics, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let assistant = assistant_db.get_assistant(assistant_id).map_err(|e| e.to_string())?;

    if assistant.assistant_type != Some(4) {
        return Err("Assistant is not ACP type".to_string());
    }

    let assistant_model_configs =
        assistant_db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;
    let assistant_models =
        assistant_db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;
    let provider_id = resolve_acp_provider_id(&assistant_models, &assistant_model_configs)
        .ok_or_else(|| "ACP assistant has no provider configured".to_string())?;

    let llm_db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let provider_configs =
        llm_db.get_llm_provider_config(provider_id).map_err(|e| e.to_string())?;

    let proxy_enabled = provider_configs
        .iter()
        .find(|config| config.name == "proxy_enabled")
        .and_then(|config| config.value.parse::<bool>().ok())
        .unwrap_or(false);

    let feature_config_state = app_handle.state::<FeatureConfigState>();
    let config_feature_map = feature_config_state.config_feature_map.lock().await.clone();
    let network_proxy =
        proxy_enabled.then(|| get_network_proxy_from_config(&config_feature_map)).flatten();

    let mut acp_config = extract_acp_config(&assistant_model_configs, &provider_configs)
        .map_err(|e| e.to_string())?;
    let explicit_proxy_env_keys =
        acp_config.env_vars.keys().filter(|key| is_proxy_env_key(key)).cloned().collect::<Vec<_>>();

    let injected_proxy_env_count = if let Some(proxy_url) = network_proxy.as_deref() {
        apply_network_proxy_to_env_vars(&mut acp_config.env_vars, proxy_url)
    } else {
        0
    };

    let resolved_cli_path = resolve_acp_cli_path(&acp_config.cli_command);
    let launch_plan = build_acp_launch_plan(
        &acp_config.cli_command,
        &resolved_cli_path,
        &acp_config.additional_args,
        &acp_config.env_vars,
    );
    let mut proxy_env = acp_config
        .env_vars
        .iter()
        .filter(|(key, _)| is_proxy_env_key(key))
        .map(|(key, value)| AcpProxyEnvEntry { key: key.clone(), value: value.clone() })
        .collect::<Vec<_>>();
    proxy_env.sort_by(|a, b| a.key.cmp(&b.key));

    let mut all_env_keys = acp_config.env_vars.keys().cloned().collect::<Vec<_>>();
    all_env_keys.sort();

    let version_probe = {
        let resolved_cli_path = resolved_cli_path.clone();
        let working_directory = acp_config.working_directory.clone();
        let env_vars: HashMap<String, String> = acp_config.env_vars.clone();
        tokio::task::spawn_blocking(move || {
            match std::process::Command::new(&resolved_cli_path)
                .arg("--version")
                .current_dir(&working_directory)
                .envs(&env_vars)
                .output()
            {
                Ok(output) => AcpCommandProbe {
                    success: output.status.success(),
                    exit_code: output.status.code(),
                    stdout: trim_probe_output(&output.stdout),
                    stderr: trim_probe_output(&output.stderr),
                },
                Err(error) => AcpCommandProbe {
                    success: false,
                    exit_code: None,
                    stdout: String::new(),
                    stderr: error.to_string(),
                },
            }
        })
        .await
        .map_err(|e| e.to_string())?
    };

    let mut notes = Vec::new();
    if !proxy_enabled {
        notes.push("provider proxy_enabled=false，当前不会注入全局网络代理".to_string());
    }
    if proxy_enabled && network_proxy.is_none() {
        notes.push("provider 已启用代理，但全局 network_proxy 为空".to_string());
    }
    if proxy_enabled && network_proxy.is_some() && injected_proxy_env_count == 0 {
        notes.push(
            "未注入新的代理环境变量，可能是你已在 ACP 环境变量里显式配置了 proxy env".to_string(),
        );
    }
    if !version_probe.success {
        notes.push("CLI --version 探测失败，请优先确认 ACP CLI 是否可执行".to_string());
    }

    Ok(AcpLaunchDiagnostics {
        assistant_id,
        provider_id,
        cli_command: acp_config.cli_command,
        resolved_cli_path: resolved_cli_path.display().to_string(),
        working_directory: acp_config.working_directory.display().to_string(),
        additional_args: acp_config.additional_args,
        effective_program: launch_plan.program.display().to_string(),
        effective_args: launch_plan.args,
        proxy_strategy: launch_plan.proxy_strategy,
        proxy_enabled,
        network_proxy,
        injected_proxy_env_count,
        explicit_proxy_env_keys,
        proxy_env,
        all_env_keys,
        version_probe,
        notes,
    })
}

// MCP Configuration Commands

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id, mcp_server_id, is_enabled))]
pub async fn update_assistant_mcp_config(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    mcp_server_id: i64,
    is_enabled: bool,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    assistant_db
        .upsert_assistant_mcp_config(assistant_id, mcp_server_id, is_enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id, mcp_tool_id, is_enabled, is_auto_run))]
pub async fn update_assistant_mcp_tool_config(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    mcp_tool_id: i64,
    is_enabled: bool,
    is_auto_run: bool,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    assistant_db
        .upsert_assistant_mcp_tool_config(assistant_id, mcp_tool_id, is_enabled, is_auto_run)
        .map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub async fn get_assistant_mcp_servers_with_tools(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<Vec<MCPServerWithTools>, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    let servers_data = assistant_db
        .get_assistant_mcp_servers_with_tools(assistant_id)
        .map_err(|e| e.to_string())?;

    let servers = servers_data
        .into_iter()
        .map(|(server_id, server_name, server_command, server_is_enabled, tools_data)| {
            let tools = tools_data
                .into_iter()
                .map(
                    |(
                        tool_id,
                        tool_name,
                        tool_description,
                        tool_is_enabled,
                        tool_is_auto_run,
                        tool_parameters,
                    )| {
                        MCPToolInfo {
                            id: tool_id,
                            name: tool_name,
                            description: tool_description,
                            is_enabled: tool_is_enabled,
                            is_auto_run: tool_is_auto_run,
                            parameters: tool_parameters,
                        }
                    },
                )
                .collect();

            MCPServerWithTools {
                id: server_id,
                name: server_name,
                summary: String::new(),
                command: server_command,
                is_enabled: server_is_enabled,
                tools,
            }
        })
        .collect();

    Ok(servers)
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id, mcp_server_id, is_enabled, is_auto_run))]
pub async fn bulk_update_assistant_mcp_tools(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    mcp_server_id: i64,
    is_enabled: bool,
    is_auto_run: Option<bool>,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Get all tools for this server from the optimized method
    let servers_data = assistant_db
        .get_assistant_mcp_servers_with_tools(assistant_id)
        .map_err(|e| e.to_string())?;

    // Find the specific server and get its tools
    let tools_data = servers_data
        .into_iter()
        .find(|(server_id, _, _, _, _)| *server_id == mcp_server_id)
        .map(|(_, _, _, _, tools)| tools)
        .unwrap_or_default();

    // Update each tool
    for (tool_id, _, _, _, current_auto_run, _) in tools_data {
        let auto_run = is_auto_run.unwrap_or(current_auto_run);
        assistant_db
            .upsert_assistant_mcp_tool_config(assistant_id, tool_id, is_enabled, auto_run)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[tauri::command]
#[instrument(skip(app_handle, config_name, config_value, value_type), fields(assistant_id, config = config_name))]
pub async fn update_assistant_model_config_value(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    config_name: String,
    config_value: String,
    value_type: String,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // 首先尝试查找是否已存在该配置
    let existing_configs =
        assistant_db.get_assistant_model_configs(assistant_id).map_err(|e| e.to_string())?;

    if let Some(existing_config) = existing_configs.iter().find(|c| c.name == config_name) {
        // 更新现有配置
        assistant_db
            .update_assistant_model_config(existing_config.id, &config_name, &config_value)
            .map_err(|e| e.to_string())?;
    } else {
        // 创建新配置 - 需要获取assistant_model_id
        let models = assistant_db.get_assistant_model(assistant_id).map_err(|e| e.to_string())?;

        let model_id = if let Some(model) = models.first() {
            model.id
        } else {
            // 如果没有模型，创建一个默认模型
            assistant_db.add_assistant_model(assistant_id, 0, "", "").map_err(|e| e.to_string())?
        };

        assistant_db
            .add_assistant_model_config(
                assistant_id,
                model_id,
                &config_name,
                &config_value,
                &value_type,
            )
            .map_err(|e| e.to_string())?;
    }

    let _ = app_handle.emit("assistant_list_changed", ());
    Ok(())
}

// Share and Import Assistant Commands

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub async fn export_assistant(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<String, String> {
    let assistant_detail = get_assistant(app_handle, assistant_id)?;

    // Convert to share format (exclude model information)
    let share_data = AssistantShareData {
        name: assistant_detail.assistant.name.clone(),
        description: assistant_detail.assistant.description.clone(),
        assistant_type: assistant_detail.assistant.assistant_type.unwrap_or(0),
        prompt: assistant_detail.prompts.first().map(|p| p.prompt.clone()).unwrap_or_default(),
        model_configs: assistant_detail
            .model_configs
            .iter()
            .map(|config| ModelConfigShare {
                name: config.name.clone(),
                value: config.value.clone().unwrap_or_default(),
                value_type: config.value_type.clone(),
            })
            .collect(),
    };

    let shared_assistant = SharedAssistant {
        version: "1.0".to_string(),
        data_type: "assistant".to_string(),
        data: share_data,
    };

    compress_assistant_data(&shared_assistant).map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(app_handle, share_code, new_name), fields(has_new_name = new_name.is_some()))]
pub async fn import_assistant(
    app_handle: tauri::AppHandle,
    share_code: String,
    new_name: Option<String>,
) -> Result<AssistantDetail, String> {
    // Decompress and validate share code
    let shared_assistant = decompress_assistant_data(&share_code).map_err(|e| e.to_string())?;

    if shared_assistant.data_type != "assistant" {
        return Err("Invalid share code: not an assistant".to_string());
    }

    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;

    // Use provided name or original name with suffix
    let assistant_name =
        new_name.unwrap_or_else(|| format!("{} (导入)", shared_assistant.data.name));

    // Create new assistant
    let new_assistant_id = assistant_db
        .add_assistant(
            &assistant_name,
            &shared_assistant.data.description.unwrap_or_default(),
            Some(shared_assistant.data.assistant_type),
            false,
        )
        .map_err(|e| e.to_string())?;

    // Add prompt
    assistant_db
        .add_assistant_prompt(new_assistant_id, &shared_assistant.data.prompt)
        .map_err(|e| e.to_string())?;

    // Add default model (will need to be configured by user)
    let model_id =
        assistant_db.add_assistant_model(new_assistant_id, 0, "", "").map_err(|e| e.to_string())?;

    // Add model configs
    for config in shared_assistant.data.model_configs {
        assistant_db
            .add_assistant_model_config(
                new_assistant_id,
                model_id,
                &config.name,
                &config.value,
                &config.value_type,
            )
            .map_err(|e| e.to_string())?;
    }

    // Broadcast assistant list update
    let _ = app_handle.emit("assistant_list_changed", ());

    // Return the created assistant detail
    get_assistant(app_handle, new_assistant_id)
}

// Assistant Workspace Commands

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id))]
pub async fn get_assistant_workspaces(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
) -> Result<Vec<crate::db::assistant_db::AssistantWorkspace>, String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    assistant_db.get_assistant_workspaces(assistant_id).map_err(|e| e.to_string())
}

#[tauri::command]
#[instrument(skip(app_handle), fields(assistant_id, path))]
pub async fn add_assistant_workspace(
    app_handle: tauri::AppHandle,
    assistant_id: i64,
    path: String,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    assistant_db.add_assistant_workspace(assistant_id, &path).map_err(|e| e.to_string())?;
    info!(assistant_id, path = %path, "Added assistant workspace");
    Ok(())
}

#[tauri::command]
#[instrument(skip(app_handle), fields(id))]
pub async fn remove_assistant_workspace(
    app_handle: tauri::AppHandle,
    id: i64,
) -> Result<(), String> {
    let assistant_db = AssistantDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    assistant_db.remove_assistant_workspace(id).map_err(|e| e.to_string())?;
    info!(id, "Removed assistant workspace");
    Ok(())
}
