use super::MessageTuple;
use crate::errors::AppError;
use genai::chat::{ChatMessage, ChatRequest};
use genai::Client;
use tracing::{debug, info};

const GENERAL_SUMMARY_PROMPT: &str = r#"请将以下对话历史压缩为结构化摘要。摘要必须足够详尽，使后续对话可以无缝继续，就像没有发生压缩一样。

必须保留：
- 用户的原始需求和最终目标
- 关键决策和结论（包含原因）
- 当前进行中的工作状态
- 所有未完成的待办事项
- 重要的约束和规则
- 涉及的关键文件路径或资源标识

摘要格式（严格遵守）：
<context_summary>
## 用户目标
[一句话概括用户的核心需求]

## 已完成
- [关键成果，包含具体细节]

## 进行中
- [当前工作的精确状态]

## 关键决策
- [重要决定及其原因]

## 待办
- [待处理事项]

## 约束与规则
- [必须遵守的限制条件]
</context_summary>"#;

const BUTLER_EXTRA_PROMPT: &str = r#"

额外必须保留（总管家专属信息）：
- 所有子任务的 task_conversation_id、标题、当前状态（ACCEPTED/RUNNING/SUCCEEDED/FAILED/CANCELLED）、执行助手名
- 已完成子任务的结论摘要和关键交付物
- 待处理的权限请求或用户确认（包含请求内容）
- 当前的工作计划（todo list 完整内容和状态）
- 可用助手目录的变化

在摘要中添加以下额外段落：
## 子任务状态
| task_conversation_id | 标题 | 状态 | 执行助手 | 结论摘要 |
|---|---|---|---|---|
[按表格列出所有子任务]

## 待处理请求
- [权限请求或用户确认]"#;

/// Generate a structured summary of the conversation body via LLM.
///
/// `body_messages` is the middle segment of the conversation (between system
/// prompt and the most recent tail messages) that needs to be compressed.
pub async fn generate_summary(
    client: &Client,
    model_name: &str,
    body_messages: &[MessageTuple],
    is_butler: bool,
) -> Result<String, AppError> {
    let body_text = format_messages_for_summary(body_messages);
    let body_token_hint = body_text.len() / 3; // rough estimate

    info!(
        body_messages = body_messages.len(),
        body_chars = body_text.len(),
        body_token_hint,
        is_butler,
        "generating context compaction summary"
    );

    let system_prompt = if is_butler {
        format!("{}{}", GENERAL_SUMMARY_PROMPT, BUTLER_EXTRA_PROMPT)
    } else {
        GENERAL_SUMMARY_PROMPT.to_string()
    };

    let messages = vec![
        ChatMessage::system(system_prompt),
        ChatMessage::user(format!(
            "以下是需要压缩的对话历史（共 {} 条消息，约 {} tokens）：\n\n{}",
            body_messages.len(),
            body_token_hint,
            body_text
        )),
    ];

    let chat_request = ChatRequest::new(messages);
    let response = client
        .exec_chat(model_name, chat_request, None)
        .await
        .map_err(|e| AppError::UnknownError(format!("context compaction LLM call failed: {}", e)))?;

    let summary = response.first_text().unwrap_or("").to_string();

    // Ensure the summary is wrapped in <context_summary> tags
    let summary = if summary.contains("<context_summary>") {
        summary
    } else {
        format!("<context_summary>\n{}\n</context_summary>", summary)
    };

    debug!(
        summary_len = summary.len(),
        "context compaction summary generated"
    );

    Ok(summary)
}

/// Format messages into a readable text block for the summarization prompt.
fn format_messages_for_summary(messages: &[MessageTuple]) -> String {
    let mut parts = Vec::with_capacity(messages.len());
    for (msg_type, content, _attachments) in messages {
        let role_label = match msg_type.as_str() {
            "system" => "[系统]",
            "user" => "[用户]",
            "response" | "assistant" => "[助手]",
            "tool_result" => "[工具结果]",
            "reasoning" => "[推理]",
            _ => "[其他]",
        };
        // Limit individual message preview to avoid blowing up the summarization prompt itself
        let truncated = if content.len() > 8000 {
            // Find a safe char boundary near 8000
            let mut end = 8000;
            while end > 0 && !content.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}...（内容过长，已截取前部分）", &content[..end])
        } else {
            content.clone()
        };
        parts.push(format!("{} {}", role_label, truncated));
    }
    parts.join("\n\n---\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_messages_basic() {
        let msgs: Vec<MessageTuple> = vec![
            ("system".into(), "You are helpful.".into(), vec![]),
            ("user".into(), "Hello".into(), vec![]),
            ("response".into(), "Hi!".into(), vec![]),
        ];
        let result = format_messages_for_summary(&msgs);
        assert!(result.contains("[系统] You are helpful."));
        assert!(result.contains("[用户] Hello"));
        assert!(result.contains("[助手] Hi!"));
        assert!(result.contains("---"));
    }

    #[test]
    fn format_messages_truncates_long_content() {
        let long_content = "a".repeat(10000);
        let msgs: Vec<MessageTuple> = vec![("user".into(), long_content, vec![])];
        let result = format_messages_for_summary(&msgs);
        assert!(result.contains("已截取前 8000 字符"));
        assert!(result.len() < 9000);
    }

    #[test]
    fn butler_prompt_includes_extra() {
        let system = format!("{}{}", GENERAL_SUMMARY_PROMPT, BUTLER_EXTRA_PROMPT);
        assert!(system.contains("task_conversation_id"));
        assert!(system.contains("总管家专属"));
    }

    #[test]
    fn general_prompt_has_required_sections() {
        assert!(GENERAL_SUMMARY_PROMPT.contains("用户目标"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("已完成"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("进行中"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("关键决策"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("待办"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("<context_summary>"));
    }
}
