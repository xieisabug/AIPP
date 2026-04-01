use crate::db::conversation_db::Message;
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

const ORIGIN_FEISHU: &str = "feishu";

#[derive(Debug, Clone)]
pub struct RenderContext<'a> {
    pub channel: &'a str,
    pub relay_origin: &'a str,
}

#[derive(Debug, Clone)]
struct ExternalToolCall {
    #[allow(dead_code)]
    call_id: Option<String>,
    #[allow(dead_code)]
    llm_call_id: Option<String>,
    server_name: String,
    tool_name: String,
    parameters: Value,
}

#[derive(Debug, Clone)]
struct ExternalToolResult {
    #[allow(dead_code)]
    call_id: Option<String>,
    server_name: Option<String>,
    tool_name: Option<String>,
    parameters: Option<Value>,
    raw_parameters: Option<String>,
    success: bool,
    output: String,
    parsed_output: Option<Value>,
}

trait ToolPresenter: Send + Sync {
    fn matches(&self, server_name: &str, tool_name: &str) -> bool;
    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String>;
    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String>;
    /// Return multiple separate messages for a single tool result.
    /// When Some, each element is sent as an independent Feishu message.
    fn present_result_parts(&self, _tool_result: &ExternalToolResult) -> Option<Vec<String>> {
        None
    }
}

struct AgentToolPresenter;
struct UiInteractionToolPresenter;
struct SearchToolPresenter;
struct OperationToolPresenter;
struct ArtifactToolPresenter;
struct FallbackToolPresenter;

struct ToolPresentationRegistry {
    presenters: Vec<Box<dyn ToolPresenter>>,
    fallback: FallbackToolPresenter,
}

impl ToolPresentationRegistry {
    fn new() -> Self {
        Self {
            presenters: vec![
                Box::new(AgentToolPresenter),
                Box::new(UiInteractionToolPresenter),
                Box::new(SearchToolPresenter),
                Box::new(OperationToolPresenter),
                Box::new(ArtifactToolPresenter),
            ],
            fallback: FallbackToolPresenter,
        }
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> String {
        self.presenters
            .iter()
            .find(|presenter| presenter.matches(&tool_call.server_name, &tool_call.tool_name))
            .and_then(|presenter| presenter.present_call(tool_call))
            .unwrap_or_else(|| self.fallback.present_call(tool_call).unwrap_or_default())
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> String {
        let server_name = tool_result.server_name.as_deref().unwrap_or_default();
        let tool_name = tool_result.tool_name.as_deref().unwrap_or_default();
        self.presenters
            .iter()
            .find(|presenter| presenter.matches(server_name, tool_name))
            .and_then(|presenter| presenter.present_result(tool_result))
            .unwrap_or_else(|| self.fallback.present_result(tool_result).unwrap_or_default())
    }

    fn present_result_vec(&self, tool_result: &ExternalToolResult) -> Vec<String> {
        let server_name = tool_result.server_name.as_deref().unwrap_or_default();
        let tool_name = tool_result.tool_name.as_deref().unwrap_or_default();
        if let Some(presenter) =
            self.presenters.iter().find(|p| p.matches(server_name, tool_name))
        {
            if let Some(parts) = presenter.present_result_parts(tool_result) {
                return parts.into_iter().filter(|s| !s.trim().is_empty()).collect();
            }
            return presenter.present_result(tool_result).into_iter().collect();
        }
        self.fallback.present_result(tool_result).into_iter().collect()
    }
}

pub fn render_preview_file_result_parts_for_feishu(params: &Value) -> Vec<String> {
    let tool_result = ExternalToolResult {
        call_id: None,
        server_name: Some("UI交互工具".to_string()),
        tool_name: Some("preview_file".to_string()),
        parameters: Some(params.clone()),
        raw_parameters: None,
        success: true,
        output: String::new(),
        parsed_output: None,
    };
    UiInteractionToolPresenter.present_result_parts(&tool_result).unwrap_or_default()
}

pub fn render_message_for_external_channel(
    message: &Message,
    context: &RenderContext<'_>,
) -> Vec<String> {
    let _channel = context.channel;
    match message.message_type.as_str() {
        "system" | "reasoning" => Vec::new(),
        "user" => render_user_message(&message.content, context).into_iter().collect(),
        "response" | "assistant" => {
            render_assistant_message(&message.content).into_iter().collect()
        }
        "tool_result" => render_tool_result_message(&message.content),
        _ => sanitize_plain_text(&message.content).into_iter().collect(),
    }
}

fn render_user_message(content: &str, context: &RenderContext<'_>) -> Option<String> {
    if context.relay_origin == ORIGIN_FEISHU {
        return None;
    }

    let sanitized = sanitize_plain_text(content)?;
    Some(format!("AIPP 用户：\n{}", sanitized))
}

fn render_assistant_message(content: &str) -> Option<String> {
    let tool_calls = parse_tool_calls_from_response(content);
    let clean_text = sanitize_plain_text(content);
    let registry = ToolPresentationRegistry::new();

    let tool_lines: Vec<String> = tool_calls
        .iter()
        .map(|tool_call| registry.present_call(tool_call))
        .filter(|line| !line.trim().is_empty())
        .collect();

    match (clean_text, tool_lines.is_empty()) {
        (Some(text), true) => Some(text),
        (Some(text), false) => Some(format!("{}\n\n{}", text, tool_lines.join("\n"))),
        (None, false) => Some(tool_lines.join("\n")),
        (None, true) => None,
    }
}

fn render_tool_result_message(content: &str) -> Vec<String> {
    let Some(tool_result) = parse_tool_result(content) else {
        return Vec::new();
    };
    let registry = ToolPresentationRegistry::new();
    registry.present_result_vec(&tool_result)
}

fn parse_tool_calls_from_response(content: &str) -> Vec<ExternalToolCall> {
    let regex = Regex::new(r"(?s)<!-- MCP_TOOL_CALL:(.*?) -->").unwrap();
    let mut calls = Vec::new();

    for capture in regex.captures_iter(content) {
        let Ok(tool_data) = serde_json::from_str::<Value>(&capture[1]) else {
            continue;
        };
        let Some(server_name) = tool_data.get("server_name").and_then(Value::as_str) else {
            continue;
        };
        let Some(tool_name) = tool_data.get("tool_name").and_then(Value::as_str) else {
            continue;
        };
        let parameters = tool_data
            .get("parameters")
            .and_then(Value::as_str)
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
            .unwrap_or_else(|| Value::Object(Default::default()));

        calls.push(ExternalToolCall {
            call_id: tool_data.get("call_id").map(json_value_to_id),
            llm_call_id: tool_data
                .get("llm_call_id")
                .and_then(Value::as_str)
                .map(ToString::to_string),
            server_name: server_name.to_string(),
            tool_name: tool_name.to_string(),
            parameters,
        });
    }

    calls
}

fn parse_tool_result(content: &str) -> Option<ExternalToolResult> {
    let success = if content.contains("Tool execution completed:") {
        true
    } else if content.contains("Tool execution failed:") {
        false
    } else {
        return None;
    };

    let output = extract_labeled_block(content, if success { "Result:\n" } else { "Error:\n" })
        .unwrap_or_else(|| content.to_string())
        .trim()
        .to_string();
    let parsed_output = serde_json::from_str::<Value>(output.trim()).ok();
    let raw_parameters = extract_labeled_line(content, "Parameters: ");
    let parameters =
        raw_parameters.as_deref().and_then(|value| serde_json::from_str::<Value>(value).ok());

    Some(ExternalToolResult {
        call_id: extract_labeled_line(content, "Tool Call ID: "),
        server_name: extract_labeled_line(content, "Server: "),
        tool_name: extract_labeled_line(content, "Tool: "),
        parameters,
        raw_parameters,
        success,
        output,
        parsed_output,
    })
}

fn extract_labeled_line(content: &str, prefix: &str) -> Option<String> {
    content.lines().find_map(|line| {
        line.strip_prefix(prefix)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
    })
}

fn extract_labeled_block(content: &str, prefix: &str) -> Option<String> {
    let start = content.find(prefix)?;
    let start_pos = start + prefix.len();
    Some(content[start_pos..].to_string())
}

fn sanitize_plain_text(content: &str) -> Option<String> {
    let tool_comment_regex = Regex::new(r"(?s)<!--\s*MCP_TOOL_CALL:.*?-->").unwrap();
    let file_attachment_regex = Regex::new(
        r#"(?s)<fileattachment\b([^>]*)>.*?</fileattachment>|<fileattachment\b([^>]*)\s*/?>"#,
    )
    .unwrap();
    let skill_attachment_regex = Regex::new(
        r#"(?s)<skillattachment\b([^>]*)>.*?</skillattachment>|<skillattachment\b([^>]*)\s*/?>"#,
    )
    .unwrap();
    let generic_attachment_regex =
        Regex::new(r#"(?s)<attachment\b([^>]*)>.*?</attachment>|<attachment\b([^>]*)\s*/?>"#)
            .unwrap();

    let without_tool_comments = tool_comment_regex.replace_all(content, "");
    let with_file_summaries = file_attachment_regex.replace_all(
        &without_tool_comments,
        |captures: &regex::Captures<'_>| {
            summarize_file_attachment(
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
            )
        },
    );
    let with_skill_summaries = skill_attachment_regex.replace_all(
        &with_file_summaries,
        |captures: &regex::Captures<'_>| {
            summarize_skill_attachment(
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
            )
        },
    );
    let without_attachments = generic_attachment_regex.replace_all(
        &with_skill_summaries,
        |captures: &regex::Captures<'_>| {
            summarize_generic_attachment(
                captures
                    .get(1)
                    .or_else(|| captures.get(2))
                    .map(|value| value.as_str())
                    .unwrap_or_default(),
            )
        },
    );
    let normalized = without_attachments.lines().map(str::trim_end).collect::<Vec<_>>().join("\n");

    collapse_blank_lines(&normalized)
}

fn collapse_blank_lines(content: &str) -> Option<String> {
    let mut output = String::new();
    let mut last_blank = false;

    for line in content.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank {
            if last_blank {
                continue;
            }
            last_blank = true;
            if !output.is_empty() {
                output.push('\n');
            }
            continue;
        }

        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(line.trim_end());
        last_blank = false;
    }

    let trimmed = output.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn normalize_server_name(server_name: &str) -> &str {
    server_name.strip_prefix("aipp:").unwrap_or(server_name)
}

fn canonical_server_name(server_name: &str) -> String {
    let normalized = normalize_server_name(server_name).trim();
    let compact = normalized
        .chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '_' && *ch != '-')
        .collect::<String>()
        .to_lowercase();
    if compact.contains("agent") {
        "agent".to_string()
    } else if compact.contains("ui交互")
        || compact.contains("uiinteraction")
        || compact.contains("ui互动")
    {
        "ui_interaction".to_string()
    } else if compact.contains("search") || compact.contains("搜索") {
        "search".to_string()
    } else if compact.contains("operation") || compact.contains("操作") {
        "operation".to_string()
    } else if compact.contains("artifact") {
        "artifact".to_string()
    } else {
        normalized.to_string()
    }
}

fn json_value_to_id(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

fn value_string<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).filter(|value| !value.trim().is_empty())
}

fn value_len(value: &Value, key: &str) -> usize {
    value.get(key).and_then(Value::as_array).map(Vec::len).unwrap_or(0)
}

fn value_u64(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}

fn preview_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.trim().chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        preview
    }
}

fn preview_multiline(value: &str, max_lines: usize, max_chars: usize) -> String {
    let lines = value
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .take(max_lines)
        .collect::<Vec<_>>()
        .join("\n");
    preview_text(&lines, max_chars)
}

fn split_content_for_feishu(content: &str, max_chars: usize) -> Vec<String> {
    if max_chars == 0 {
        return vec![content.to_string()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_len = 0usize;

    for line in content.split_inclusive('\n') {
        let line_len = line.chars().count();
        if current_len + line_len <= max_chars {
            current.push_str(line);
            current_len += line_len;
            continue;
        }

        if !current.is_empty() {
            chunks.push(current);
            current = String::new();
            current_len = 0;
        }

        if line_len <= max_chars {
            current.push_str(line);
            current_len = line_len;
            continue;
        }

        let mut line_chunk = String::new();
        let mut line_chunk_len = 0usize;
        for ch in line.chars() {
            line_chunk.push(ch);
            line_chunk_len += 1;
            if line_chunk_len == max_chars {
                chunks.push(line_chunk);
                line_chunk = String::new();
                line_chunk_len = 0;
            }
        }

        if !line_chunk.is_empty() {
            current = line_chunk;
            current_len = line_chunk_len;
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    if chunks.is_empty() {
        vec![String::new()]
    } else {
        chunks
    }
}

fn preview_part_suffix(part_index: usize, total_parts: usize) -> String {
    if total_parts > 1 {
        format!("（第 {}/{} 部分）", part_index + 1, total_parts)
    } else {
        String::new()
    }
}

fn task_action_label(action: &str) -> &str {
    match action {
        "read" => "查看对话",
        "reply_prompt" => "发送指令",
        "mcp_tool_execute" => "执行工具调用",
        "permission_confirm" | "operate_confirm" => "操作权限审批",
        "acp_permission_confirm" => "ACP 权限审批",
        "ask_user_respond" => "回复提问",
        _ => action,
    }
}

fn decision_label(decision: &str) -> &str {
    match decision {
        "allow" => "允许",
        "deny" => "拒绝",
        "allow_and_save" => "允许并记住",
        _ => decision,
    }
}

fn parse_tag_attributes(attributes: &str) -> HashMap<String, String> {
    let attribute_regex = Regex::new(r#"([A-Za-z_][\w:-]*)="([^"]*)""#).unwrap();
    attribute_regex
        .captures_iter(attributes)
        .filter_map(|captures| {
            Some((captures.get(1)?.as_str().to_string(), captures.get(2)?.as_str().to_string()))
        })
        .collect()
}

fn tag_attribute<'a>(attributes: &'a HashMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| attributes.get(*key).map(String::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn basename_like(value: &str) -> String {
    value.rsplit(|ch| ch == '/' || ch == '\\').next().unwrap_or(value).trim().to_string()
}

fn looks_like_image(value: &str) -> bool {
    let lowered = value.to_lowercase();
    [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".svg"]
        .iter()
        .any(|suffix| lowered.ends_with(suffix))
}

fn looks_like_document(value: &str) -> bool {
    let lowered = value.to_lowercase();
    [".pdf", ".doc", ".docx", ".xls", ".xlsx", ".ppt", ".pptx", ".txt", ".md", ".csv", ".json"]
        .iter()
        .any(|suffix| lowered.ends_with(suffix))
}

fn summarize_file_attachment(attributes: &str) -> String {
    let attributes = parse_tag_attributes(attributes);
    let display_name = tag_attribute(
        &attributes,
        &["name", "title", "filename", "display_name", "displayName", "attachment_url", "url"],
    )
    .map(basename_like)
    .unwrap_or_else(|| "未命名附件".to_string());
    let attachment_type =
        tag_attribute(&attributes, &["attachment_type", "type"]).unwrap_or_default().to_lowercase();
    let kind = if attachment_type == "image" || looks_like_image(&display_name) {
        "图片附件"
    } else if attachment_type == "file"
        || attachment_type == "document"
        || looks_like_document(&display_name)
    {
        "文件附件"
    } else {
        "附件"
    };
    format!("[{}] {}", kind, display_name)
}

fn summarize_skill_attachment(attributes: &str) -> String {
    let attributes = parse_tag_attributes(attributes);
    let skill_name =
        tag_attribute(&attributes, &["skill_name", "name", "title"]).unwrap_or("未命名技能");
    match tag_attribute(&attributes, &["identifier", "skill_identifier"]) {
        Some(identifier) => format!("[技能附件] {}\n标识符：{}", skill_name, identifier),
        None => format!("[技能附件] {}", skill_name),
    }
}

fn summarize_generic_attachment(attributes: &str) -> String {
    let attributes = parse_tag_attributes(attributes);
    let display_name = tag_attribute(
        &attributes,
        &["name", "title", "display_name", "displayName", "attachment_url", "url", "path"],
    )
    .map(basename_like)
    .unwrap_or_else(|| "未命名附件".to_string());
    let attachment_type =
        tag_attribute(&attributes, &["type", "attachment_type"]).unwrap_or("附件");
    format!("[{}] {}", attachment_type, display_name)
}

fn json_array<'a>(value: &'a Value, key: &str) -> Option<&'a [Value]> {
    value.get(key).and_then(Value::as_array).map(Vec::as_slice)
}

fn tool_result_parameters(tool_result: &ExternalToolResult) -> Option<Value> {
    tool_result.parameters.clone().or_else(|| {
        tool_result
            .raw_parameters
            .as_deref()
            .and_then(|value| serde_json::from_str::<Value>(value).ok())
    })
}

fn tool_result_content_items(tool_result: &ExternalToolResult) -> Option<&[Value]> {
    match tool_result.parsed_output.as_ref()? {
        Value::Array(items) => Some(items.as_slice()),
        Value::Object(object) => object.get("content").and_then(Value::as_array).map(Vec::as_slice),
        _ => None,
    }
}

fn tool_result_content_text(tool_result: &ExternalToolResult) -> Option<&str> {
    tool_result_content_items(tool_result)?.iter().find_map(|item| {
        match item.get("type").and_then(Value::as_str) {
            Some("text") => item.get("text").and_then(Value::as_str),
            _ => None,
        }
    })
}

fn tool_result_content_json(tool_result: &ExternalToolResult) -> Option<&Value> {
    tool_result_content_items(tool_result)?.iter().find_map(|item| {
        match item.get("type").and_then(Value::as_str) {
            Some("json") => item.get("json"),
            _ => None,
        }
    })
}

fn tool_result_effective_success(tool_result: &ExternalToolResult) -> bool {
    if !tool_result.success {
        return false;
    }
    match tool_result.parsed_output.as_ref() {
        Some(Value::Object(value)) => {
            !value.get("isError").and_then(Value::as_bool).unwrap_or(false)
        }
        _ => true,
    }
}

fn tool_result_text_preview(tool_result: &ExternalToolResult, max_chars: usize) -> String {
    let text = tool_result_content_text(tool_result).unwrap_or(&tool_result.output);
    preview_text(text, max_chars)
}

fn preview_list<I>(items: I, max_items: usize) -> String
where
    I: IntoIterator<Item = String>,
{
    let items = items.into_iter().filter(|item| !item.trim().is_empty()).collect::<Vec<_>>();
    if items.is_empty() {
        return String::new();
    }
    let total = items.len();
    let shown = items.into_iter().take(max_items).collect::<Vec<_>>();
    if total > shown.len() {
        format!("{} 等 {} 项", shown.join("、"), total)
    } else {
        shown.join("、")
    }
}

fn summarize_parameter_schema(parameters: &Value) -> Option<String> {
    let properties = parameters.get("properties")?.as_object()?;
    if properties.is_empty() {
        return None;
    }
    let required = parameters
        .get("required")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    let mut entries = Vec::new();
    for (name, definition) in properties.iter().take(4) {
        let mut label = if required.contains(name) {
            format!("{}（必填）", name)
        } else {
            name.to_string()
        };
        if let Some(description) = value_string(definition, "description") {
            label.push('：');
            label.push_str(&preview_text(description, 36));
        }
        entries.push(label);
    }
    let remaining = properties.len().saturating_sub(entries.len());
    if remaining > 0 {
        entries.push(format!("另有 {} 个参数", remaining));
    }
    Some(entries.join("；"))
}

fn summarize_dynamic_mcp_servers(servers: &[Value]) -> String {
    let mut lines = vec![format!("🔌 已加载 {} 个工具集", servers.len())];
    lines.push(String::new());
    for server in servers.iter().take(4) {
        let name = value_string(server, "server").unwrap_or("未知工具集");
        let summary = value_string(server, "summary")
            .map(|value| preview_text(value, 72))
            .unwrap_or_else(|| "暂无摘要".to_string());
        let tools = json_array(server, "tools").unwrap_or(&[]);
        let tool_names = preview_list(
            tools.iter().filter_map(|tool| value_string(tool, "tool").map(ToString::to_string)),
            3,
        );
        let tool_segment = if tool_names.is_empty() {
            format!("{} 个工具", tools.len())
        } else {
            format!("{} 个工具：{}", tools.len(), tool_names)
        };
        lines.push(format!("📦 {}（{}）", name, tool_segment));
        lines.push(format!("  {}", summary));
    }
    if servers.len() > 4 {
        lines.push(format!("另有 {} 个工具集未展开", servers.len() - 4));
    }
    lines.join("\n")
}

fn summarize_loaded_mcp_tools(tools: &[Value]) -> String {
    let mut lines = vec![format!("🔧 已加载 {} 个工具说明", tools.len())];
    lines.push(String::new());
    for tool in tools.iter().take(4) {
        let server = value_string(tool, "server").unwrap_or("未知服务");
        let tool_name = value_string(tool, "tool").unwrap_or("未知工具");
        let description = value_string(tool, "description")
            .map(|value| preview_text(value, 80))
            .unwrap_or_else(|| "暂无用途摘要".to_string());
        lines.push(format!("• {}/{}：{}", server, tool_name, description));
        if let Some(summary) = tool.get("parameters").and_then(summarize_parameter_schema) {
            lines.push(format!("  参数：{}", summary));
        }
    }
    if tools.len() > 4 {
        lines.push(format!("另有 {} 个工具未展开", tools.len() - 4));
    }
    lines.join("\n")
}

fn summarize_todo_write(params: &Value) -> Option<String> {
    let todos = json_array(params, "todos")?;
    if todos.is_empty() {
        return Some("📋 任务列表更新（空列表）".to_string());
    }

    let mut completed = 0usize;
    let mut todo_lines = Vec::new();

    for todo in todos {
        let status = value_string(todo, "status").unwrap_or("pending");
        let content = value_string(todo, "content").unwrap_or("未命名任务");
        let active_form =
            value_string(todo, "activeForm").or_else(|| value_string(todo, "active_form"));
        let icon = match status {
            "completed" | "done" => {
                completed += 1;
                "✅"
            }
            "in_progress" => "⏳",
            "blocked" => "🚫",
            _ => "⬜",
        };
        let line = match active_form {
            Some(form) if form != content => format!("{} {} → {}", icon, content, form),
            _ => format!("{} {}", icon, content),
        };
        todo_lines.push(line);
    }

    let mut lines = vec![format!("📋 任务列表更新（{} 项，{} 完成）", todos.len(), completed)];
    lines.push(String::new());
    lines.extend(todo_lines);
    Some(lines.join("\n"))
}

fn preview_file_kind_label(file_type: &str) -> &'static str {
    match file_type {
        "markdown" | "text" => "文本",
        "image" => "图片",
        "pdf" => "PDF",
        "html" => "HTML",
        _ => "文件",
    }
}

fn summarize_preview_items(value: &Value, include_excerpt: bool) -> Option<String> {
    let files = json_array(value, "files")?;
    if files.is_empty() {
        return None;
    }
    if files.len() > 1 {
        let titles = preview_list(
            files.iter().map(|file| {
                let title = value_string(file, "title").unwrap_or("未命名文件");
                let file_type = value_string(file, "type").unwrap_or("file");
                format!("{}（{}）", title, preview_file_kind_label(file_type))
            }),
            4,
        );
        return Some(format!("{} 个文件：{}", files.len(), titles));
    }

    let file = &files[0];
    let title = value_string(file, "title").unwrap_or("未命名文件");
    let file_type = value_string(file, "type").unwrap_or("file");
    let mut lines = vec![format!("{}「{}」", preview_file_kind_label(file_type), title)];
    if matches!(file_type, "markdown" | "text") {
        if let Some(language) = value_string(file, "language") {
            lines.push(format!("语言：{}", language));
        }
        if include_excerpt {
            if let Some(content) = value_string(file, "content") {
                lines.push(format!("内容摘要：\n{}", preview_multiline(content, 6, 220)));
            }
        }
    }
    if let Some(description) = value_string(file, "description") {
        lines.push(format!("说明：{}", preview_text(description, 120)));
    }
    Some(lines.join("\n"))
}

fn summarize_spawn_task_result(tool_result: &ExternalToolResult) -> Option<String> {
    let payload = tool_result_content_json(tool_result)?;
    let title = value_string(payload, "title")
        .or_else(|| payload.get("task").and_then(|t| value_string(t, "title")))
        .unwrap_or("未命名子任务");
    let executor = value_string(payload, "executor_assistant_name")
        .or_else(|| value_string(payload, "executor_assistant_id"));
    let status = payload
        .get("task")
        .and_then(|t| value_string(t, "status"))
        .or_else(|| value_string(payload, "status"))
        .unwrap_or("pending");
    let task_id = payload
        .get("task_conversation_id")
        .and_then(Value::as_u64)
        .or_else(|| payload.get("task").and_then(|t| value_u64(t, "id")));

    let mut lines = vec![format!("✅ 子任务已创建「{}」", title)];
    if let Some(executor) = executor {
        lines.push(format!("执行助手：{}", executor));
    }
    lines.push(format!("状态：{}", status));
    if let Some(id) = task_id {
        lines.push(format!("任务 ID：{}", id));
    }
    Some(lines.join("\n"))
}

fn summarize_execute_bash_result(tool_result: &ExternalToolResult) -> String {
    let params = tool_result_parameters(tool_result);
    let label = params
        .as_ref()
        .and_then(|value| value_string(value, "description"))
        .or_else(|| params.as_ref().and_then(|value| value_string(value, "command")))
        .map(|value| preview_text(value, 120))
        .unwrap_or_else(|| "命令".to_string());
    let text = tool_result_content_text(tool_result).unwrap_or(&tool_result.output);
    let is_background =
        text.contains("background") && (text.contains("bash_id") || text.contains("bash-"));

    if is_background {
        let mut lines = vec![format!("⚡ 后台命令已启动：{}", label)];
        lines.push(preview_multiline(text, 4, 200));
        return lines.join("\n");
    }

    let success = tool_result_effective_success(tool_result);
    let mut lines = vec![format!(
        "{} {}：{}",
        if success { "⚡" } else { "❌" },
        if success { "命令完成" } else { "命令失败" },
        label,
    )];
    let output = preview_multiline(text, 10, 260);
    if !output.is_empty() {
        lines.push(format!("输出：\n{}", output));
    }
    lines.join("\n")
}

fn summarize_bash_output_result(tool_result: &ExternalToolResult) -> String {
    let params = tool_result_parameters(tool_result);
    let bash_id =
        params.as_ref().and_then(|value| value_string(value, "bash_id")).unwrap_or("未知任务");
    let output = tool_result_content_text(tool_result)
        .map(|value| preview_multiline(value, 10, 260))
        .unwrap_or_else(|| tool_result_text_preview(tool_result, 260));
    let mut lines = vec![format!("📤 命令输出（{}）", bash_id)];
    if !output.is_empty() {
        lines.push(output);
    }
    lines.join("\n")
}

fn summarize_read_file_result(tool_result: &ExternalToolResult) -> Option<String> {
    let params = tool_result_parameters(tool_result)?;
    let file_path = value_string(&params, "file_path").unwrap_or("未知文件");
    let text = tool_result_content_text(tool_result).unwrap_or("");
    let line_count = text.lines().count();
    let mut lines = vec![format!("📖 已读取 {}", file_path)];
    if line_count > 0 {
        lines.push(format!("共 {} 行", line_count));
    }
    Some(lines.join("\n"))
}

fn summarize_write_file_result(tool_result: &ExternalToolResult) -> Option<String> {
    let params = tool_result_parameters(tool_result)?;
    let file_path = value_string(&params, "file_path").unwrap_or("未知文件");
    let text =
        tool_result_content_text(tool_result).map(|v| preview_text(v, 140)).unwrap_or_default();
    let mut lines = vec![format!("✏️ 已写入 {}", file_path)];
    if !text.is_empty() {
        lines.push(text);
    }
    Some(lines.join("\n"))
}

fn summarize_edit_file_result(tool_result: &ExternalToolResult) -> Option<String> {
    let params = tool_result_parameters(tool_result)?;
    let file_path = value_string(&params, "file_path").unwrap_or("未知文件");
    let text =
        tool_result_content_text(tool_result).map(|v| preview_text(v, 140)).unwrap_or_default();
    let mut lines = vec![format!("📝 已编辑 {}", file_path)];
    if !text.is_empty() {
        lines.push(text);
    }
    Some(lines.join("\n"))
}

fn present_task_conversation_operation_call(params: &Value) -> Option<String> {
    let task_id = value_u64(params, "task_conversation_id")
        .map(|id| id.to_string())
        .unwrap_or_else(|| "?".to_string());
    let action = value_string(params, "action").unwrap_or("未知操作");

    match action {
        "read" => {
            let count = value_u64(params, "latest_count").unwrap_or(1);
            if count > 1 {
                Some(format!("📖 查看子任务 #{} 最新 {} 条对话", task_id, count))
            } else {
                Some(format!("📖 查看子任务 #{} 的对话进展", task_id))
            }
        }
        "reply_prompt" => {
            let prompt = value_string(params, "prompt")
                .map(|p| format!("\n{}", preview_text(p, 100)))
                .unwrap_or_default();
            Some(format!("💬 向子任务 #{} 发送指令{}", task_id, prompt))
        }
        "mcp_tool_execute" => Some(format!("⚡ 批准子任务 #{} 的工具调用执行", task_id)),
        "permission_confirm" | "operate_confirm" => {
            let d = value_string(params, "decision")
                .map(decision_label)
                .unwrap_or("未知");
            Some(format!("🔐 审批子任务 #{} 的操作权限：{}", task_id, d))
        }
        "acp_permission_confirm" => {
            let cancelled = params.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
            if cancelled {
                Some(format!("🔐 取消子任务 #{} 的 ACP 权限请求", task_id))
            } else {
                let option_id = value_string(params, "option_id").unwrap_or("未知选项");
                Some(format!(
                    "🔐 审批子任务 #{} 的 ACP 权限：选择「{}」",
                    task_id, option_id
                ))
            }
        }
        "ask_user_respond" => {
            let cancelled = params.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
            if cancelled {
                Some(format!("💬 取消子任务 #{} 的用户提问", task_id))
            } else {
                let answers = params.get("answers").and_then(Value::as_object);
                match answers {
                    Some(map) if !map.is_empty() => {
                        let answer_summary = preview_list(
                            map.iter().map(|(k, v)| {
                                let val = v.as_str().unwrap_or("");
                                format!("{}={}", k, preview_text(val, 40))
                            }),
                            3,
                        );
                        Some(format!(
                            "💬 回复子任务 #{} 的提问：{}",
                            task_id, answer_summary
                        ))
                    }
                    _ => Some(format!("💬 回复子任务 #{} 的提问", task_id)),
                }
            }
        }
        _ => Some(format!("📋 操作子任务 #{} — {}", task_id, action)),
    }
}

fn present_task_conversation_operation_result(tool_result: &ExternalToolResult) -> Option<String> {
    let params = tool_result_parameters(tool_result);
    let task_id = params
        .as_ref()
        .and_then(|p| value_u64(p, "task_conversation_id"))
        .map(|id| id.to_string())
        .unwrap_or_else(|| "?".to_string());
    let action = params
        .as_ref()
        .and_then(|p| value_string(p, "action"))
        .unwrap_or("未知操作");

    if !tool_result_effective_success(tool_result) {
        return Some(format!(
            "❌ 子任务 #{} {}失败\n{}",
            task_id,
            task_action_label(action),
            tool_result_text_preview(tool_result, 180)
        ));
    }

    match action {
        "read" => Some(format!("📖 已查看子任务 #{} 的对话进展", task_id)),
        "reply_prompt" => {
            let prompt = params
                .as_ref()
                .and_then(|p| value_string(p, "prompt"))
                .map(|p| format!("\n{}", preview_text(p, 100)))
                .unwrap_or_default();
            Some(format!("💬 已向子任务 #{} 发送指令{}", task_id, prompt))
        }
        "mcp_tool_execute" => {
            Some(format!("⚡ 已派发子任务 #{} 的工具调用执行", task_id))
        }
        "permission_confirm" | "operate_confirm" => {
            let json = tool_result_content_json(tool_result);
            let d = json
                .and_then(|j| value_string(j, "decision"))
                .map(decision_label)
                .unwrap_or("未知");
            Some(format!("🔐 已审批子任务 #{} 的操作权限：{}", task_id, d))
        }
        "acp_permission_confirm" => {
            let json = tool_result_content_json(tool_result);
            let cancelled = json
                .and_then(|j| j.get("cancelled"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if cancelled {
                Some(format!("🔐 已取消子任务 #{} 的 ACP 权限请求", task_id))
            } else {
                Some(format!("🔐 已审批子任务 #{} 的 ACP 权限", task_id))
            }
        }
        "ask_user_respond" => {
            let json = tool_result_content_json(tool_result);
            let cancelled = json
                .and_then(|j| j.get("cancelled"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if cancelled {
                Some(format!("💬 已取消子任务 #{} 的用户提问", task_id))
            } else {
                Some(format!("💬 已回复子任务 #{} 的提问", task_id))
            }
        }
        _ => Some(format!(
            "📋 子任务 #{} {} 完成",
            task_id,
            task_action_label(action)
        )),
    }
}

impl ToolPresenter for AgentToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        canonical_server_name(server_name) == "agent"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "todo_write" => summarize_todo_write(&tool_call.parameters),
            "spawn_task_conversation" => {
                let title = value_string(&tool_call.parameters, "title").unwrap_or("未命名任务");
                let executor = value_string(&tool_call.parameters, "executor_assistant_name")
                    .or_else(|| value_string(&tool_call.parameters, "executor_assistant_id"));
                let first_line = match executor {
                    Some(executor) => format!("🔀 派发子任务「{}」→ {}", title, executor),
                    None => format!("🔀 派发子任务「{}」", title),
                };
                let goal = value_string(&tool_call.parameters, "goal")
                    .map(|value| format!("\n目标：{}", preview_text(value, 120)));
                Some(format!("{}{}", first_line, goal.unwrap_or_default()))
            }
            "load_skill" => {
                let identifier =
                    value_string(&tool_call.parameters, "identifier").unwrap_or("未知技能");
                Some(format!("💡 加载技能：{}", identifier))
            }
            "load_mcp_server" => {
                let keyword = value_string(&tool_call.parameters, "name").unwrap_or("关键词");
                Some(format!("🔌 加载工具集：{}", preview_text(keyword, 80)))
            }
            "load_mcp_tool" => {
                let names = json_array(&tool_call.parameters, "names")
                    .map(|values| {
                        preview_list(
                            values.iter().filter_map(Value::as_str).map(ToString::to_string),
                            3,
                        )
                    })
                    .filter(|value| !value.is_empty())
                    .or_else(|| {
                        value_string(&tool_call.parameters, "name")
                            .map(|value| preview_text(value, 80))
                    })
                    .unwrap_or_else(|| "关键词".to_string());
                Some(format!("🔧 加载工具说明：{}", names))
            }
            "task_conversation_operation" => {
                present_task_conversation_operation_call(&tool_call.parameters)
            }
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("load_mcp_server") if tool_result_effective_success(tool_result) => {
                tool_result_content_json(tool_result)
                    .and_then(|payload| json_array(payload, "servers"))
                    .map(summarize_dynamic_mcp_servers)
                    .or_else(|| {
                        Some(format!(
                            "🔌 已加载工具集：{}",
                            tool_result_text_preview(tool_result, 180)
                        ))
                    })
            }
            Some("load_mcp_server") => Some(format!(
                "❌ 未找到匹配的工具集：{}",
                tool_result_text_preview(tool_result, 180)
            )),
            Some("load_mcp_tool") if tool_result_effective_success(tool_result) => {
                tool_result_content_json(tool_result)
                    .and_then(|payload| json_array(payload, "tools"))
                    .map(summarize_loaded_mcp_tools)
                    .or_else(|| {
                        Some(format!(
                            "🔧 已加载工具说明：{}",
                            tool_result_text_preview(tool_result, 180)
                        ))
                    })
            }
            Some("load_mcp_tool") => {
                Some(format!("❌ 工具说明加载失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            Some("todo_write") if tool_result_effective_success(tool_result) => {
                tool_result_parameters(tool_result).as_ref().and_then(summarize_todo_write).or_else(
                    || {
                        Some(format!(
                            "📋 任务列表已同步：{}",
                            tool_result_text_preview(tool_result, 180)
                        ))
                    },
                )
            }
            Some("todo_write") => {
                Some(format!("❌ 任务列表更新失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            Some("spawn_task_conversation") if tool_result_effective_success(tool_result) => {
                summarize_spawn_task_result(tool_result).or_else(|| {
                    Some(format!("✅ 子任务已创建：{}", tool_result_text_preview(tool_result, 180)))
                })
            }
            Some("spawn_task_conversation") => {
                Some(format!("❌ 子任务创建失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            Some("load_skill") if tool_result_effective_success(tool_result) => {
                let params = tool_result_parameters(tool_result);
                let identifier = params
                    .as_ref()
                    .and_then(|p| value_string(p, "identifier"))
                    .unwrap_or("未知技能");
                Some(format!("💡 技能已加载：{}", identifier))
            }
            Some("load_skill") => {
                Some(format!("❌ 技能加载失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            Some("task_conversation_operation") => {
                present_task_conversation_operation_result(tool_result)
            }
            _ => None,
        }
    }
}

impl ToolPresenter for UiInteractionToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        canonical_server_name(server_name) == "ui_interaction"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "ask_user_question" => {
                let questions = json_array(&tool_call.parameters, "questions").unwrap_or(&[]);
                let count_suffix = if questions.len() > 1 {
                    format!("（{} 个问题）", questions.len())
                } else {
                    String::new()
                };
                let first_q = questions
                    .first()
                    .and_then(|q| value_string(q, "question"))
                    .map(|q| format!("\n{}", preview_text(q, 120)))
                    .unwrap_or_default();
                Some(format!("❓ 等待用户回答{}{}", count_suffix, first_q))
            }
            "preview_file" => summarize_preview_items(&tool_call.parameters, false)
                .map(|summary| format!("👁 预览{}", summary))
                .or_else(|| {
                    Some(format!(
                        "👁 预览 {} 个文件",
                        value_len(&tool_call.parameters, "files").max(1)
                    ))
                }),
            "preview_code" => {
                let title = value_string(&tool_call.parameters, "title")
                    .map(|value| preview_text(value, 80))
                    .unwrap_or_else(|| "内嵌交互界面".to_string());
                let renderer = value_string(&tool_call.parameters, "renderer").unwrap_or("html");
                Some(format!("🎨 展示内嵌 UI「{}」({})", title, renderer))
            }
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("ask_user_question") if tool_result.success => {
                Some("✅ 已收到用户回答".to_string())
            }
            Some("ask_user_question") => Some("❌ 用户回答获取失败".to_string()),
            Some("preview_file") if tool_result_effective_success(tool_result) => {
                let params = tool_result_parameters(tool_result)?;
                summarize_preview_items(&params, true)
                    .map(|summary| format!("👁 已展示{}", summary))
                    .or_else(|| Some("👁 预览内容已展示".to_string()))
            }
            Some("preview_file") => {
                Some(format!("❌ 预览展示失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            Some("preview_code") if tool_result_effective_success(tool_result) => {
                let result = tool_result_content_json(tool_result);
                let status = result
                    .as_ref()
                    .and_then(|value| value_string(value, "status"))
                    .unwrap_or("submitted");
                match status {
                    "dismissed" => Some("🎨 内嵌 UI 已关闭".to_string()),
                    "submitted" => Some("🎨 已收到内嵌 UI 提交结果".to_string()),
                    other => Some(format!("🎨 内嵌 UI 已完成：{}", other)),
                }
            }
            Some("preview_code") => {
                Some(format!("❌ 内嵌 UI 处理失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            _ => None,
        }
    }

    fn present_result_parts(&self, tool_result: &ExternalToolResult) -> Option<Vec<String>> {
        if tool_result.tool_name.as_deref() != Some("preview_file") {
            return None;
        }
        if !tool_result_effective_success(tool_result) {
            return None;
        }
        let params = tool_result_parameters(tool_result)?;
        let files = json_array(&params, "files")?;
        if files.is_empty() {
            return None;
        }

        let mut parts = Vec::new();
        for file in files {
            let title = value_string(file, "title").unwrap_or("未命名文件");
            let file_type = value_string(file, "type").unwrap_or("file");
            let content = value_string(file, "content");

            match content {
                Some(content) if !content.trim().is_empty() => {
                    let is_markdown = matches!(file_type, "markdown")
                        || title.to_lowercase().ends_with(".md");
                    let language = value_string(file, "language").unwrap_or("");
                    if is_markdown {
                        let content_parts = split_content_for_feishu(content, 3800);
                        let total_parts = content_parts.len();
                        for (part_index, content_part) in content_parts.into_iter().enumerate() {
                            let header = format!(
                                "**📄 {}{}**\n\n",
                                title,
                                preview_part_suffix(part_index, total_parts)
                            );
                            parts.push(format!("{}{}", header, content_part));
                        }
                    } else if language.is_empty()
                        || language == "text"
                        || language == "plaintext"
                    {
                        let label = preview_file_kind_label(file_type);
                        let content_parts = split_content_for_feishu(content, 3600);
                        let total_parts = content_parts.len();
                        for (part_index, content_part) in content_parts.into_iter().enumerate() {
                            parts.push(format!(
                                "**📄 {}{}**（{}）\n\n```\n{}\n```",
                                title,
                                preview_part_suffix(part_index, total_parts),
                                label,
                                content_part
                            ));
                        }
                    } else {
                        let label = preview_file_kind_label(file_type);
                        let content_parts = split_content_for_feishu(content, 3600);
                        let total_parts = content_parts.len();
                        for (part_index, content_part) in content_parts.into_iter().enumerate() {
                            parts.push(format!(
                                "**📄 {}{}**（{}）\n\n```{}\n{}\n```",
                                title,
                                preview_part_suffix(part_index, total_parts),
                                label,
                                language,
                                content_part
                            ));
                        }
                    }
                }
                _ => {
                    let label = preview_file_kind_label(file_type);
                    if let Some(description) = value_string(file, "description") {
                        parts.push(format!(
                            "📄 {}（{}）\n{}",
                            title,
                            label,
                            preview_text(description, 120)
                        ));
                    } else {
                        parts.push(format!("📄 {}（{}）", title, label));
                    }
                }
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts)
        }
    }
}

impl ToolPresenter for SearchToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        canonical_server_name(server_name) == "search"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "search_web" => value_string(&tool_call.parameters, "query")
                .map(|query| format!("🔍 搜索：{}", preview_text(query, 120))),
            "fetch_url" => value_string(&tool_call.parameters, "url")
                .map(|url| format!("🌐 抓取：{}", preview_text(url, 120))),
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("search_web") if tool_result.success => {
                let params = tool_result_parameters(tool_result);
                let query =
                    params.as_ref().and_then(|p| value_string(p, "query")).unwrap_or("搜索");
                if let Some(json) = tool_result_content_json(tool_result) {
                    if let Some(items) = json_array(json, "items") {
                        let mut lines = vec![format!("🔍 搜索完成：{}", preview_text(query, 80))];
                        lines.push(format!("找到 {} 条结果", items.len()));
                        for item in items.iter().take(3) {
                            let title = value_string(item, "title").unwrap_or("无标题");
                            let url = value_string(item, "display_url")
                                .or_else(|| value_string(item, "url"))
                                .unwrap_or("");
                            if url.is_empty() {
                                lines.push(format!("• {}", preview_text(title, 80)));
                            } else {
                                lines.push(format!(
                                    "• {} - {}",
                                    preview_text(title, 60),
                                    preview_text(url, 60)
                                ));
                            }
                        }
                        if items.len() > 3 {
                            lines.push(format!("另有 {} 条结果", items.len() - 3));
                        }
                        return Some(lines.join("\n"));
                    }
                }
                Some(format!(
                    "🔍 搜索完成：{}\n{}",
                    preview_text(query, 80),
                    tool_result_text_preview(tool_result, 260)
                ))
            }
            Some("search_web") => {
                Some(format!("❌ 搜索失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            Some("fetch_url") if tool_result.success => {
                let params = tool_result_parameters(tool_result);
                let url = params.as_ref().and_then(|p| value_string(p, "url")).unwrap_or("网页");
                Some(format!(
                    "🌐 网页已抓取：{}\n{}",
                    preview_text(url, 100),
                    tool_result_text_preview(tool_result, 260)
                ))
            }
            Some("fetch_url") => {
                let params = tool_result_parameters(tool_result);
                let url = params.as_ref().and_then(|p| value_string(p, "url")).unwrap_or("网页");
                Some(format!("❌ 网页抓取失败：{}", preview_text(url, 120)))
            }
            _ => None,
        }
    }
}

impl ToolPresenter for OperationToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        canonical_server_name(server_name) == "operation"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "read_file" => value_string(&tool_call.parameters, "file_path")
                .map(|path| format!("📖 读取 {}", path)),
            "write_file" => value_string(&tool_call.parameters, "file_path")
                .map(|path| format!("✏️ 写入 {}", path)),
            "edit_file" => value_string(&tool_call.parameters, "file_path")
                .map(|path| format!("📝 编辑 {}", path)),
            "list_directory" => value_string(&tool_call.parameters, "path")
                .map(|path| format!("📂 列出目录 {}", path)),
            "execute_bash" => {
                let description = value_string(&tool_call.parameters, "description");
                let command = value_string(&tool_call.parameters, "command");
                Some(match (description, command) {
                    (Some(description), _) => {
                        format!("⚡ {}", preview_text(description, 120))
                    }
                    (None, Some(command)) => format!("⚡ {}", preview_text(command, 120)),
                    _ => "⚡ 执行命令".to_string(),
                })
            }
            "get_bash_output" => value_string(&tool_call.parameters, "bash_id")
                .map(|bash_id| format!("📤 读取命令输出：{}", bash_id)),
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        let params = tool_result_parameters(tool_result);
        match tool_result.tool_name.as_deref() {
            Some("read_file") if tool_result_effective_success(tool_result) => {
                summarize_read_file_result(tool_result).or_else(|| {
                    params
                        .as_ref()
                        .and_then(|p| value_string(p, "file_path"))
                        .map(|path| format!("📖 已读取 {}", path))
                })
            }
            Some("read_file") => {
                let path = params.as_ref().and_then(|p| value_string(p, "file_path"));
                Some(match path {
                    Some(path) => format!(
                        "❌ 读取失败 {}\n{}",
                        path,
                        tool_result_text_preview(tool_result, 180)
                    ),
                    None => format!("❌ 读取失败：{}", tool_result_text_preview(tool_result, 180)),
                })
            }
            Some("write_file") if tool_result_effective_success(tool_result) => {
                summarize_write_file_result(tool_result).or_else(|| {
                    params
                        .as_ref()
                        .and_then(|p| value_string(p, "file_path"))
                        .map(|path| format!("✏️ 已写入 {}", path))
                })
            }
            Some("write_file") => {
                let path = params.as_ref().and_then(|p| value_string(p, "file_path"));
                Some(match path {
                    Some(path) => format!(
                        "❌ 写入失败 {}\n{}",
                        path,
                        tool_result_text_preview(tool_result, 180)
                    ),
                    None => format!("❌ 写入失败：{}", tool_result_text_preview(tool_result, 180)),
                })
            }
            Some("edit_file") if tool_result_effective_success(tool_result) => {
                summarize_edit_file_result(tool_result).or_else(|| {
                    params
                        .as_ref()
                        .and_then(|p| value_string(p, "file_path"))
                        .map(|path| format!("📝 已编辑 {}", path))
                })
            }
            Some("edit_file") => {
                let path = params.as_ref().and_then(|p| value_string(p, "file_path"));
                Some(match path {
                    Some(path) => format!(
                        "❌ 编辑失败 {}\n{}",
                        path,
                        tool_result_text_preview(tool_result, 180)
                    ),
                    None => format!("❌ 编辑失败：{}", tool_result_text_preview(tool_result, 180)),
                })
            }
            Some("list_directory") if tool_result_effective_success(tool_result) => params
                .as_ref()
                .and_then(|p| value_string(p, "path"))
                .map(|path| format!("📂 已获取目录列表：{}", path)),
            Some("list_directory") => params
                .as_ref()
                .and_then(|p| value_string(p, "path"))
                .map(|path| format!("❌ 目录列表获取失败：{}", path)),
            Some("execute_bash") => Some(summarize_execute_bash_result(tool_result)),
            Some("get_bash_output") if tool_result_effective_success(tool_result) => {
                Some(summarize_bash_output_result(tool_result))
            }
            Some("get_bash_output") => {
                Some(format!("❌ 读取命令输出失败：{}", tool_result_text_preview(tool_result, 180)))
            }
            _ => None,
        }
    }
}

impl ToolPresenter for ArtifactToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        canonical_server_name(server_name) == "artifact"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "get_artifact_workspace" => Some("📦 读取 Artifact 工作区".to_string()),
            "show_artifact" => {
                let artifact_id = value_string(&tool_call.parameters, "artifactId")
                    .or_else(|| value_string(&tool_call.parameters, "artifact_id"))
                    .unwrap_or("未知 Artifact");
                Some(format!("🎨 展示 Artifact：{}", artifact_id))
            }
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("show_artifact") if tool_result.success => {
                let params = tool_result_parameters(tool_result);
                let artifact_id = params
                    .as_ref()
                    .and_then(|p| {
                        value_string(p, "artifactId").or_else(|| value_string(p, "artifact_id"))
                    })
                    .unwrap_or("Artifact");
                Some(format!("🎨 Artifact 已展示：{}", artifact_id))
            }
            Some("show_artifact") => Some("❌ Artifact 展示失败".to_string()),
            Some("get_artifact_workspace") if tool_result.success => {
                Some("📦 Artifact 工作区已读取".to_string())
            }
            Some("get_artifact_workspace") => Some("❌ Artifact 工作区读取失败".to_string()),
            _ => None,
        }
    }
}

impl ToolPresenter for FallbackToolPresenter {
    fn matches(&self, _server_name: &str, _tool_name: &str) -> bool {
        true
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        let server_name = normalize_server_name(&tool_call.server_name);
        Some(format!("🔧 {}/{}", server_name, tool_call.tool_name))
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        let server_name =
            tool_result.server_name.as_deref().map(normalize_server_name).unwrap_or("unknown");
        let tool_name = tool_result.tool_name.as_deref().unwrap_or("unknown");
        let (icon, prefix) = if tool_result_effective_success(tool_result) {
            ("✅", "执行完成")
        } else {
            ("❌", "执行失败")
        };
        let summary = tool_result_content_text(tool_result)
            .map(|value| preview_multiline(value, 8, 220))
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| preview_text(&tool_result.output, 220));
        if summary.is_empty() {
            Some(format!("{} {}/{} {}", icon, server_name, tool_name, prefix))
        } else {
            Some(format!("{} {}/{} {}\n{}", icon, server_name, tool_name, prefix, summary))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render_message_for_external_channel, RenderContext};
    use crate::db::conversation_db::Message;
    use chrono::Utc;
    use serde_json::{json, Value};

    fn build_message(message_type: &str, content: &str) -> Message {
        Message {
            id: 1,
            parent_id: None,
            conversation_id: 1,
            message_type: message_type.to_string(),
            content: content.to_string(),
            llm_model_id: None,
            llm_model_name: None,
            created_time: Utc::now(),
            start_time: None,
            finish_time: Some(Utc::now()),
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

    fn build_tool_result_message(
        tool: &str,
        server: &str,
        parameters: Value,
        result: Value,
    ) -> Message {
        build_message(
            "tool_result",
            &format!(
                "Tool execution completed:\n\nTool Call ID: call_1\nTool: {}\nServer: {}\nParameters: {}\nResult:\n{}",
                tool,
                server,
                parameters,
                result
            ),
        )
    }

    /// Join all rendered parts for assertion convenience.
    fn render_joined(message: &Message, context: &RenderContext<'_>) -> Option<String> {
        let parts = render_message_for_external_channel(message, context);
        if parts.is_empty() {
            None
        } else {
            Some(parts.join("\n"))
        }
    }

    #[test]
    fn renders_response_without_raw_mcp_comments() {
        let message = build_message(
            "response",
            "先检查一下。<!-- MCP_TOOL_CALL:{\"server_name\":\"aipp:operation\",\"tool_name\":\"read_file\",\"parameters\":\"{\\\"file_path\\\":\\\"C:\\\\\\\\demo.txt\\\"}\"} -->",
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("先检查一下"));
        assert!(rendered.contains("📖 读取 C:\\demo.txt"));
        assert!(!rendered.contains("MCP_TOOL_CALL"));
    }

    #[test]
    fn skips_feishu_origin_user_echo() {
        let message = build_message("user", "你好");
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "feishu" },
        );

        assert!(rendered.is_none());
    }

    #[test]
    fn renders_tool_result_readably() {
        let message = build_message(
            "tool_result",
            "Tool execution completed:\n\nTool Call ID: call_1\nTool: search_web\nServer: aipp:search\nParameters: {\"query\":\"rust tauri\"}\nResult:\n找到 5 条结果",
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("🔍 搜索完成"));
        assert!(!rendered.contains("Tool execution completed"));
    }

    #[test]
    fn summarizes_skill_and_file_attachments_without_leaking_body() {
        let message = build_message(
            "user",
            "请看附件\n<fileattachment name=\"diagram.png\">data:image/png;base64,AAAA</fileattachment>\n<skillattachment skill_name=\"skill-creator\" invocation=\"/skills(skill-creator)\" identifier=\"agents:skill-creator\"># hidden prompt\nsecret body</skillattachment>",
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("[图片附件] diagram.png"));
        assert!(rendered.contains("[技能附件] skill-creator"));
        assert!(rendered.contains("标识符：agents:skill-creator"));
        assert!(!rendered.contains("base64"));
        assert!(!rendered.contains("secret body"));
        assert!(!rendered.contains("/skills(skill-creator)"));
    }

    #[test]
    fn summarizes_load_mcp_server_result() {
        let message = build_tool_result_message(
            "load_mcp_server",
            "Agent 工具",
            json!({"name": "github"}),
            json!([{
                "type": "json",
                "json": {
                    "servers": [{
                        "server": "GitHub",
                        "summary": "GitHub 协作工具集",
                        "tools": [
                            {"tool": "issue_read", "summary": "读取 issue"},
                            {"tool": "pull_request_read", "summary": "读取 PR"}
                        ]
                    }]
                }
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("🔌 已加载 1 个工具集"));
        assert!(rendered.contains("GitHub"));
        assert!(rendered.contains("2 个工具"));
        assert!(rendered.contains("issue_read"));
        assert!(!rendered.contains("\"servers\""));
    }

    #[test]
    fn summarizes_load_mcp_tool_result() {
        let message = build_tool_result_message(
            "load_mcp_tool",
            "Agent 工具",
            json!({"names": ["search_code"]}),
            json!([{
                "type": "json",
                "json": {
                    "tools": [{
                        "server": "GitHub",
                        "tool": "search_code",
                        "description": "搜索仓库中的代码。",
                        "parameters": {
                            "type": "object",
                            "properties": {
                                "query": {"type": "string", "description": "搜索语句"},
                                "limit": {"type": "integer", "description": "返回结果数量"}
                            },
                            "required": ["query"]
                        }
                    }]
                }
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("🔧 已加载 1 个工具说明"));
        assert!(rendered.contains("GitHub/search_code"));
        assert!(rendered.contains("query（必填）"));
        assert!(rendered.contains("limit"));
    }

    #[test]
    fn summarizes_todo_write_result() {
        let message = build_tool_result_message(
            "todo_write",
            "Agent 工具",
            json!({
                "todos": [
                    {"content": "修复飞书展示", "status": "completed", "activeForm": "修复飞书展示"},
                    {"content": "创建CSIC数据集下载脚本", "status": "in_progress", "activeForm": "创建CSIC数据集下载脚本中"},
                    {"content": "补充验证", "status": "pending", "activeForm": "补充验证"}
                ]
            }),
            json!([{
                "type": "text",
                "text": "Todo list updated: 1/3 tasks completed (33%)\n\nCurrent task: 创建CSIC数据集下载脚本中"
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("📋 任务列表更新（3 项，1 完成）"));
        assert!(rendered.contains("✅ 修复飞书展示"));
        assert!(rendered.contains("⏳ 创建CSIC数据集下载脚本 → 创建CSIC数据集下载脚本中"));
        assert!(rendered.contains("⬜ 补充验证"));
        assert!(!rendered.contains("Todo list updated"));
    }

    #[test]
    fn summarizes_preview_and_bash_results() {
        let preview_message = build_tool_result_message(
            "preview_file",
            "UI交互工具",
            json!({
                "files": [{
                    "title": "README.md",
                    "type": "markdown",
                    "language": "markdown",
                    "content": "# Title\nline1\nline2"
                }]
            }),
            json!([{
                "type": "json",
                "json": {"status": "preview_shown", "request_id": "req_1"}
            }]),
        );
        let preview_parts = render_message_for_external_channel(
            &preview_message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        );
        assert_eq!(preview_parts.len(), 1);
        assert!(preview_parts[0].contains("README.md"));
        assert!(preview_parts[0].contains("# Title\nline1\nline2"));

        let bash_message = build_tool_result_message(
            "execute_bash",
            "操作工具",
            json!({"description": "安装依赖", "command": "npm install"}),
            json!([{
                "type": "text",
                "text": "Command started in background. Use get_bash_output with bash_id='bash-1' to check output."
            }]),
        );
        let bash_rendered = render_joined(
            &bash_message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();
        assert!(bash_rendered.contains("⚡ 后台命令已启动：安装依赖"));
    }

    #[test]
    fn summarizes_read_file_result() {
        let message = build_tool_result_message(
            "read_file",
            "操作工具",
            json!({"file_path": "C:\\demo.txt"}),
            json!([{
                "type": "text",
                "text": "     1\tfirst line\n     2\tsecond line"
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("📖 已读取 C:\\demo.txt"));
        assert!(rendered.contains("共 2 行"));
    }

    #[test]
    fn tool_calls_use_icons_not_executing_prefix() {
        let message = build_message(
            "response",
            "好的，我来处理。<!-- MCP_TOOL_CALL:{\"server_name\":\"Agent 工具\",\"tool_name\":\"todo_write\",\"parameters\":\"{\\\"todos\\\":[{\\\"content\\\":\\\"修复bug\\\",\\\"status\\\":\\\"in_progress\\\",\\\"activeForm\\\":\\\"修复bug中\\\"}]}\"} --><!-- MCP_TOOL_CALL:{\"server_name\":\"aipp:operation\",\"tool_name\":\"read_file\",\"parameters\":\"{\\\"file_path\\\":\\\"src/main.rs\\\"}\"} -->",
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(!rendered.contains("正在执行"));
        assert!(rendered.contains("📋 任务列表更新"));
        assert!(rendered.contains("⏳ 修复bug → 修复bug中"));
        assert!(rendered.contains("📖 读取 src/main.rs"));
    }

    #[test]
    fn search_web_result_shows_items() {
        let message = build_tool_result_message(
            "search_web",
            "搜索工具",
            json!({"query": "Rust 异步编程"}),
            json!([{
                "type": "json",
                "json": {
                    "query": "Rust 异步编程",
                    "items": [
                        {"title": "Async Rust 指南", "url": "https://example.com/async", "display_url": "example.com/async"},
                        {"title": "Tokio 教程", "url": "https://tokio.rs", "display_url": "tokio.rs"}
                    ]
                }
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("🔍 搜索完成"));
        assert!(rendered.contains("找到 2 条结果"));
        assert!(rendered.contains("Async Rust 指南"));
        assert!(rendered.contains("Tokio 教程"));
    }

    #[test]
    fn fallback_presenter_uses_icon_format() {
        let message = build_tool_result_message(
            "custom_tool",
            "外部工具",
            json!({}),
            json!([{"type": "text", "text": "执行成功"}]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("✅ 外部工具/custom_tool 执行完成"));
        assert!(rendered.contains("执行成功"));
        assert!(!rendered.contains("工具执行完成"));
    }

    #[test]
    fn write_file_result_shows_icon() {
        let message = build_tool_result_message(
            "write_file",
            "操作工具",
            json!({"file_path": "/tmp/output.txt", "content": "hello"}),
            json!([{"type": "text", "text": "Successfully wrote 5 bytes to /tmp/output.txt"}]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("✏️ 已写入 /tmp/output.txt"));
        assert!(rendered.contains("Successfully wrote 5 bytes"));
    }

    #[test]
    fn spawn_task_result_shows_structured_info() {
        let message = build_tool_result_message(
            "spawn_task_conversation",
            "Agent 工具",
            json!({"title": "数据分析", "executor_assistant_name": "分析助手"}),
            json!([{
                "type": "json",
                "json": {
                    "task_conversation_id": 42,
                    "task": {"id": 99, "title": "数据分析", "status": "pending"},
                    "message": "Task created"
                }
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("✅ 子任务已创建「数据分析」"));
        assert!(rendered.contains("状态：pending"));
        assert!(rendered.contains("任务 ID：42"));
    }

    #[test]
    fn preview_file_multi_files_returns_multiple_parts() {
        let message = build_tool_result_message(
            "preview_file",
            "UI交互工具",
            json!({
                "files": [
                    {
                        "title": "README.md",
                        "type": "markdown",
                        "language": "markdown",
                        "content": "# Hello\nWorld"
                    },
                    {
                        "title": "main.rs",
                        "type": "text",
                        "language": "rust",
                        "content": "fn main() {}"
                    }
                ]
            }),
            json!([{
                "type": "json",
                "json": {"status": "preview_shown", "request_id": "req_2"}
            }]),
        );
        let parts = render_message_for_external_channel(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        );
        assert_eq!(parts.len(), 2);
        assert!(parts[0].contains("README.md"));
        assert!(parts[0].contains("# Hello\nWorld"));
        assert!(parts[1].contains("main.rs"));
        assert!(parts[1].contains("fn main() {}"));
        assert!(parts[1].contains("```rust"));
    }

    #[test]
    fn preview_file_long_content_is_split_without_truncation() {
        let long_content = format!(
            "{}\n{}\nEND",
            "A".repeat(3700),
            "B".repeat(3700),
        );
        let message = build_tool_result_message(
            "preview_file",
            "UI交互工具",
            json!({
                "files": [{
                    "title": "main.rs",
                    "type": "text",
                    "language": "rust",
                    "content": long_content
                }]
            }),
            json!([{
                "type": "json",
                "json": {"status": "preview_shown", "request_id": "req_long"}
            }]),
        );

        let parts = render_message_for_external_channel(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        );

        assert!(parts.len() >= 3);
        assert!(parts[0].contains("第 1/"));
        assert!(parts.iter().all(|part| !part.contains("内容过长，已截断")));
        assert!(parts[0].contains(&"A".repeat(100)));
        assert!(parts.iter().any(|part| part.contains(&"B".repeat(100))));
        assert!(parts.iter().any(|part| part.contains("END")));
    }

    #[test]
    fn task_conversation_operation_call_is_human_readable() {
        let message = build_message(
            "response",
            "<!-- MCP_TOOL_CALL:{\"server_name\":\"Agent 工具\",\"tool_name\":\"task_conversation_operation\",\"parameters\":\"{\\\"task_conversation_id\\\":42,\\\"action\\\":\\\"reply_prompt\\\",\\\"prompt\\\":\\\"请继续分析数据\\\"}\"} -->",
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("💬 向子任务 #42 发送指令"));
        assert!(rendered.contains("请继续分析数据"));
        assert!(!rendered.contains("task_conversation_operation"));
    }

    #[test]
    fn task_conversation_operation_result_is_human_readable() {
        let message = build_tool_result_message(
            "task_conversation_operation",
            "Agent 工具",
            json!({"task_conversation_id": 42, "action": "permission_confirm", "decision": "allow"}),
            json!([{
                "type": "json",
                "json": {
                    "status": "permission_confirmed",
                    "request_id": "req_1",
                    "decision": "allow",
                    "resolved": true
                }
            }]),
        );
        let rendered = render_joined(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("🔐 已审批子任务 #42 的操作权限：允许"));
        assert!(!rendered.contains("permission_confirmed"));
    }
}
