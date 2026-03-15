use crate::db::conversation_db::Message;
use regex::Regex;
use serde_json::Value;

const ORIGIN_FEISHU: &str = "feishu";

#[derive(Debug, Clone)]
pub struct RenderContext<'a> {
    pub channel: &'a str,
    pub relay_origin: &'a str,
}

#[derive(Debug, Clone)]
struct ExternalToolCall {
    call_id: Option<String>,
    llm_call_id: Option<String>,
    server_name: String,
    tool_name: String,
    parameters: Value,
}

#[derive(Debug, Clone)]
struct ExternalToolResult {
    call_id: Option<String>,
    server_name: Option<String>,
    tool_name: Option<String>,
    parameters: Option<Value>,
    raw_parameters: Option<String>,
    success: bool,
    output: String,
}

trait ToolPresenter: Send + Sync {
    fn matches(&self, server_name: &str, tool_name: &str) -> bool;
    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String>;
    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String>;
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
}

pub fn render_message_for_external_channel(
    message: &Message,
    context: &RenderContext<'_>,
) -> Option<String> {
    let _channel = context.channel;
    match message.message_type.as_str() {
        "system" | "reasoning" => None,
        "user" => render_user_message(&message.content, context),
        "response" | "assistant" => render_assistant_message(&message.content),
        "tool_result" => render_tool_result_message(&message.content),
        _ => sanitize_plain_text(&message.content),
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
        (Some(text), false) => Some(format!(
            "{}\n\n正在执行：\n{}",
            text,
            tool_lines.into_iter().map(|line| format!("- {}", line)).collect::<Vec<_>>().join("\n")
        )),
        (None, false) => Some(format!(
            "正在执行：\n{}",
            tool_lines.into_iter().map(|line| format!("- {}", line)).collect::<Vec<_>>().join("\n")
        )),
        (None, true) => None,
    }
}

fn render_tool_result_message(content: &str) -> Option<String> {
    let tool_result = parse_tool_result(content)?;
    let registry = ToolPresentationRegistry::new();
    let rendered = registry.present_result(&tool_result);
    if rendered.trim().is_empty() {
        None
    } else {
        Some(rendered)
    }
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
    let attachment_regex =
        Regex::new(r#"(?s)<fileattachment name="([^"]+)">.*?</fileattachment>"#).unwrap();

    let without_tool_comments = tool_comment_regex.replace_all(content, "");
    let without_attachments = attachment_regex.replace_all(&without_tool_comments, "[附件: $1]");
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

fn preview_text(value: &str, max_chars: usize) -> String {
    let mut chars = value.trim().chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{}...", preview)
    } else {
        preview
    }
}

impl ToolPresenter for AgentToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        normalize_server_name(server_name) == "agent"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "spawn_task_conversation" => {
                let title = value_string(&tool_call.parameters, "title").unwrap_or("未命名任务");
                let executor = value_string(&tool_call.parameters, "executor_assistant_name")
                    .or_else(|| value_string(&tool_call.parameters, "executor_assistant_id"));
                let goal = value_string(&tool_call.parameters, "goal")
                    .map(|value| format!("目标：{}", preview_text(value, 120)))
                    .unwrap_or_default();
                Some(match executor {
                    Some(executor) if !goal.is_empty() => {
                        format!("派发子任务「{}」给 {}。{}", title, executor, goal)
                    }
                    Some(executor) => format!("派发子任务「{}」给 {}", title, executor),
                    None if !goal.is_empty() => format!("派发子任务「{}」。{}", title, goal),
                    None => format!("派发子任务「{}」", title),
                })
            }
            "load_skill" => {
                let identifier =
                    value_string(&tool_call.parameters, "identifier").unwrap_or("未知技能");
                Some(format!("加载技能 {}", identifier))
            }
            "todo_write" => Some("更新执行 Todo 清单".to_string()),
            "load_mcp_server" => {
                let names = value_len(&tool_call.parameters, "names");
                Some(format!("加载 {} 个 MCP 服务", names.max(1)))
            }
            "load_mcp_tool" => {
                let names = value_len(&tool_call.parameters, "names");
                Some(format!("加载 {} 个 MCP 工具", names.max(1)))
            }
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("spawn_task_conversation") if tool_result.success => {
                Some(format!("任务派发完成：{}", preview_text(&tool_result.output, 180)))
            }
            Some("spawn_task_conversation") => {
                Some(format!("任务派发失败：{}", preview_text(&tool_result.output, 180)))
            }
            _ => None,
        }
    }
}

impl ToolPresenter for UiInteractionToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        normalize_server_name(server_name) == "ui_interaction"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "ask_user_question" => {
                let count = value_len(&tool_call.parameters, "questions").max(1);
                Some(format!("向用户发起 {} 个补充问题", count))
            }
            "preview_file" => {
                let count = value_len(&tool_call.parameters, "files").max(1);
                Some(format!("展示 {} 个预览项", count))
            }
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("ask_user_question") if tool_result.success => {
                Some(format!("已收到用户补充信息：{}", preview_text(&tool_result.output, 180)))
            }
            Some("preview_file") if tool_result.success => Some("预览内容已展示".to_string()),
            _ => None,
        }
    }
}

impl ToolPresenter for SearchToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        normalize_server_name(server_name) == "search"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "search_web" => value_string(&tool_call.parameters, "query")
                .map(|query| format!("搜索网页：{}", preview_text(query, 120))),
            "fetch_url" => value_string(&tool_call.parameters, "url")
                .map(|url| format!("抓取网页：{}", preview_text(url, 120))),
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("search_web") if tool_result.success => {
                Some(format!("网页搜索完成：{}", preview_text(&tool_result.output, 180)))
            }
            Some("fetch_url") if tool_result.success => {
                Some(format!("网页抓取完成：{}", preview_text(&tool_result.output, 180)))
            }
            _ => None,
        }
    }
}

impl ToolPresenter for OperationToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        normalize_server_name(server_name) == "operation"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "read_file" => value_string(&tool_call.parameters, "file_path")
                .map(|path| format!("读取文件 {}", path)),
            "write_file" => value_string(&tool_call.parameters, "file_path")
                .map(|path| format!("写入文件 {}", path)),
            "edit_file" => value_string(&tool_call.parameters, "file_path")
                .map(|path| format!("编辑文件 {}", path)),
            "list_directory" => {
                value_string(&tool_call.parameters, "path").map(|path| format!("列出目录 {}", path))
            }
            "execute_bash" => {
                let description = value_string(&tool_call.parameters, "description");
                let command = value_string(&tool_call.parameters, "command");
                Some(match (description, command) {
                    (Some(description), _) => {
                        format!("执行命令：{}", preview_text(description, 120))
                    }
                    (None, Some(command)) => format!("执行命令：{}", preview_text(command, 120)),
                    _ => "执行命令".to_string(),
                })
            }
            "get_bash_output" => value_string(&tool_call.parameters, "bash_id")
                .map(|bash_id| format!("读取后台命令输出 {}", bash_id)),
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        let params = tool_result.parameters.clone().or_else(|| {
            tool_result
                .raw_parameters
                .as_deref()
                .and_then(|value| serde_json::from_str::<Value>(value).ok())
        });
        match tool_result.tool_name.as_deref() {
            Some("read_file") if tool_result.success => params
                .as_ref()
                .and_then(|params| value_string(params, "file_path"))
                .map(|path| format!("文件读取完成：{}", path)),
            Some("write_file") if tool_result.success => params
                .as_ref()
                .and_then(|params| value_string(params, "file_path"))
                .map(|path| format!("文件写入完成：{}", path)),
            Some("edit_file") if tool_result.success => params
                .as_ref()
                .and_then(|params| value_string(params, "file_path"))
                .map(|path| format!("文件编辑完成：{}", path)),
            Some("list_directory") if tool_result.success => params
                .as_ref()
                .and_then(|params| value_string(params, "path"))
                .map(|path| format!("目录列表已获取：{}", path)),
            Some("execute_bash") if tool_result.success => {
                Some(format!("命令执行完成：{}", preview_text(&tool_result.output, 180)))
            }
            Some("execute_bash") => {
                Some(format!("命令执行失败：{}", preview_text(&tool_result.output, 180)))
            }
            Some("get_bash_output") if tool_result.success => {
                Some(format!("后台命令输出：{}", preview_text(&tool_result.output, 180)))
            }
            _ => None,
        }
    }
}

impl ToolPresenter for ArtifactToolPresenter {
    fn matches(&self, server_name: &str, _tool_name: &str) -> bool {
        normalize_server_name(server_name) == "artifact"
    }

    fn present_call(&self, tool_call: &ExternalToolCall) -> Option<String> {
        match tool_call.tool_name.as_str() {
            "get_artifact_workspace" => Some("读取 Artifact 工作区".to_string()),
            "show_artifact" => {
                let artifact_id = value_string(&tool_call.parameters, "artifactId")
                    .or_else(|| value_string(&tool_call.parameters, "artifact_id"))
                    .unwrap_or("未知 Artifact");
                Some(format!("展示 Artifact {}", artifact_id))
            }
            _ => None,
        }
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        match tool_result.tool_name.as_deref() {
            Some("show_artifact") if tool_result.success => Some("Artifact 已展示".to_string()),
            Some("get_artifact_workspace") if tool_result.success => {
                Some("Artifact 工作区已读取".to_string())
            }
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
        let call_id = tool_call
            .llm_call_id
            .as_deref()
            .or(tool_call.call_id.as_deref())
            .map(|id| format!("（调用 ID: {}）", id))
            .unwrap_or_default();
        Some(format!("调用工具 {}/{}{}", server_name, tool_call.tool_name, call_id))
    }

    fn present_result(&self, tool_result: &ExternalToolResult) -> Option<String> {
        let server_name =
            tool_result.server_name.as_deref().map(normalize_server_name).unwrap_or("unknown");
        let tool_name = tool_result.tool_name.as_deref().unwrap_or("unknown");
        let call_id = tool_result
            .call_id
            .as_deref()
            .map(|id| format!("（调用 ID: {}）", id))
            .unwrap_or_default();
        let prefix = if tool_result.success { "工具执行完成" } else { "工具执行失败" };
        if tool_result.output.trim().is_empty() {
            Some(format!("{}：{}/{}{}", prefix, server_name, tool_name, call_id))
        } else {
            Some(format!(
                "{}：{}/{}{}。\n{}",
                prefix,
                server_name,
                tool_name,
                call_id,
                preview_text(&tool_result.output, 220)
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{render_message_for_external_channel, RenderContext};
    use crate::db::conversation_db::Message;
    use chrono::Utc;

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

    #[test]
    fn renders_response_without_raw_mcp_comments() {
        let message = build_message(
            "response",
            "先检查一下。<!-- MCP_TOOL_CALL:{\"server_name\":\"aipp:operation\",\"tool_name\":\"read_file\",\"parameters\":\"{\\\"file_path\\\":\\\"C:\\\\\\\\demo.txt\\\"}\"} -->",
        );
        let rendered = render_message_for_external_channel(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("先检查一下"));
        assert!(rendered.contains("读取文件 C:\\demo.txt"));
        assert!(!rendered.contains("MCP_TOOL_CALL"));
    }

    #[test]
    fn skips_feishu_origin_user_echo() {
        let message = build_message("user", "你好");
        let rendered = render_message_for_external_channel(
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
        let rendered = render_message_for_external_channel(
            &message,
            &RenderContext { channel: "feishu", relay_origin: "aipp" },
        )
        .unwrap();

        assert!(rendered.contains("网页搜索完成"));
        assert!(!rendered.contains("Tool execution completed"));
    }
}
