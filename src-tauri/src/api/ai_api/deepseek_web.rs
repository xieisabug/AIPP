use super::*;
use genai::chat::{ChatOptions, ChatRequest, ReasoningEffort};
use crate::db::llm_db::ModelDetail;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebMode { #[default] Off, Search, Deep }

fn available(detail: &ModelDetail) -> bool {
    detail.provider.api_type == "deepseek" && detail.model.code == "deepseek-flash"
        && detail.configs.iter().any(|c| c.name == "deepseek_web_enabled" && c.value == "true")
}

pub fn selected(detail: &ModelDetail, mode: Option<&serde_json::Value>) -> WebMode {
    if !available(detail) { return WebMode::Off; }
    mode.and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default()
}

pub fn from_history(detail: &ModelDetail, user: Option<&Message>) -> WebMode {
    let metadata = user.and_then(|m| m.metadata_json.as_deref())
        .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok());
    selected(detail, metadata.as_ref().and_then(|v| v.get("deepseek_web_mode")))
}

pub fn filter_search(info: &mut MCPInfoForAssistant, mode: WebMode) -> Vec<String> {
    let mut names = Vec::new();
    if mode == WebMode::Off { return names; }
    for server in &mut info.enabled_servers {
        if server.command.as_deref() == Some("aipp:search") {
            names.push(server.name.clone());
            server.tools.retain(|t| !matches!(t.name.as_str(), "search_web" | "search_http" | "fetch_url"));
        }
    }
    info.enabled_servers.retain(|s| !s.tools.is_empty() || s.command.as_deref() != Some("aipp:search"));
    names
}

/// Older conversations already have AIPP's MCP catalogue in their saved system prompt.
fn remove_search_catalogue(prompt: &mut String, servers: &[String]) {
    if !prompt.starts_with("# 助手指令\n") { return; }
    for name in servers {
        if let Some(catalogue) = prompt.find("\n## 可用的 MCP 工具\n") {
            let header = format!("### 服务器: {name}\n");
            if let Some(start) = prompt[catalogue..].find(&header).map(|i| i + catalogue) {
                if let Some(end) = prompt[start..].find("\n---\n\n") {
                    prompt.replace_range(start..start + end + 6, "");
                }
            }
        }
        if let Some(catalogue) = prompt.find("\n## MCP 工具集目录摘要\n") {
            let prefix = format!("\n- 工具集 `{name}`：");
            if let Some(start) = prompt[catalogue..].find(&prefix).map(|i| i + catalogue) {
                let end = prompt[start+1..].find('\n').map_or(prompt.len(), |i| start+1+i);
                prompt.replace_range(start..end, "");
            }
        }
    }
}

pub fn configure(mode: WebMode, search_servers: &[String], flash: bool, request: &mut ChatRequest, options: &mut ChatOptions) {
    use genai::chat::{ChatRole, ContentPart, MessageContent};
    for message in &mut request.messages {
        if !flash && message.content.iter().any(|p| matches!(p, ContentPart::Custom(c) if c.typ() == Some("deepseek_web"))) {
            let mut parts = std::mem::take(&mut message.content).into_parts();
            parts.retain(|p| !matches!(p, ContentPart::Custom(c) if c.typ() == Some("deepseek_web")));
            message.content = MessageContent::from_parts(parts);
        }
        if flash && mode != WebMode::Off && matches!(message.role, ChatRole::System) {
            for part in message.content.iter_mut() {
                if let ContentPart::Text(prompt) = part { remove_search_catalogue(prompt, search_servers); }
            }
        }
    }
    if !flash || mode == WebMode::Off { return; }
    options.deepseek_web = Some(true);
    let thinking = mode == WebMode::Deep || request.tools.as_ref().is_some_and(|tools| !tools.is_empty());
    options.reasoning_effort = Some(if thinking { ReasoningEffort::High } else { ReasoningEffort::Zero });
    if thinking {
        options.temperature = None;
        options.top_p = None;
    }
    options.capture_reasoning_content = Some(thinking);
}

#[derive(Serialize)]
pub struct WebCapability { native_tools: bool }

#[tauri::command]
pub async fn get_deepseek_web_capability(app_handle: tauri::AppHandle, assistant_id: i64, override_model_id: Option<String>, conversation_id: Option<i64>) -> Result<Option<WebCapability>, String> {
    let assistant = get_assistant(app_handle.clone(), assistant_id)?;
    if assistant.assistant.assistant_type == Some(4) || assistant.model.is_empty() { return Ok(None); }
    let db = LLMDatabase::new(&app_handle).map_err(|e| e.to_string())?;
    if override_model_id.is_none() && assistant.model[0].model_code != "deepseek-flash" { return Ok(None); }
    let model = resolve_ask_model_detail(&db, override_model_id.as_deref(), &assistant)?;
    if !available(&model) { return Ok(None); }
    let mut info = collect_mcp_info_for_conversation(&app_handle, assistant_id, None, None, conversation_id)
        .await.map_err(|e| e.to_string())?;
    filter_search(&mut info, WebMode::Search);
    let native_tools = build_tool_config(&app_handle, &info, info.use_native_toolcall, conversation_id)
        .is_some_and(|config| !config.tools.is_empty());
    Ok(Some(WebCapability { native_tools }))
}

#[cfg(test)]
#[path = "deepseek_web_tests.rs"]
mod tests;
