use super::*;
use genai::chat::{ChatOptions, ChatRequest, ReasoningEffort};
use crate::db::llm_db::ModelDetail;

/// 对话内联网只由服务商详情里的「对话内联网」开关决定：开了之后该服务商的 deepseek-flash
/// 每轮都带上服务端搜索/阅读链接，聊天界面不再有单独的开关。
pub fn enabled(detail: &ModelDetail) -> bool {
    detail.provider.api_type == "deepseek" && detail.model.code == "deepseek-flash"
        && detail.configs.iter().any(|c| c.name == "deepseek_web_enabled" && c.value == "true")
}

pub fn filter_search(info: &mut MCPInfoForAssistant, enabled: bool) -> Vec<String> {
    let mut names = Vec::new();
    if !enabled { return names; }
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

pub fn configure(enabled: bool, search_servers: &[String], flash: bool, request: &mut ChatRequest, options: &mut ChatOptions) {
    use genai::chat::{ChatRole, ContentPart, MessageContent};
    for message in &mut request.messages {
        if !flash && message.content.iter().any(|p| matches!(p, ContentPart::Custom(c) if c.typ() == Some("deepseek_web"))) {
            let mut parts = std::mem::take(&mut message.content).into_parts();
            parts.retain(|p| !matches!(p, ContentPart::Custom(c) if c.typ() == Some("deepseek_web")));
            message.content = MessageContent::from_parts(parts);
        }
        if flash && enabled && matches!(message.role, ChatRole::System) {
            for part in message.content.iter_mut() {
                if let ContentPart::Text(prompt) = part { remove_search_catalogue(prompt, search_servers); }
            }
        }
    }
    if !flash || !enabled { return; }
    options.deepseek_web = Some(true);
    // 思考跟随助手自己的 reasoning_effort。DeepSeek 只在思考模式下允许客户端工具与服务端联网共存，
    // 所以本轮带原生工具而助手没开思考时补上 High，否则请求会 400。
    let native_tools = request.tools.as_ref().is_some_and(|tools| !tools.is_empty());
    if native_tools && matches!(options.reasoning_effort, None | Some(ReasoningEffort::Zero)) {
        options.reasoning_effort = Some(ReasoningEffort::High);
    }
    let thinking = options.reasoning_effort.as_ref().is_some_and(|effort| !matches!(effort, ReasoningEffort::Zero));
    if thinking {
        // 思考模式下 DeepSeek 忽略温度与 top-p，不发出去以免看起来生效了。
        options.temperature = None;
        options.top_p = None;
    }
    options.capture_reasoning_content = Some(!matches!(options.reasoning_effort, Some(ReasoningEffort::Zero)));
}

#[cfg(test)]
#[path = "deepseek_web_tests.rs"]
mod tests;
