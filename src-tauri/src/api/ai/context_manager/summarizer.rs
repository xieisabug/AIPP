use super::MessageTuple;
use crate::errors::AppError;
use genai::chat::{ChatMessage, ChatRequest, StopReason};
use genai::Client;
use tracing::{debug, info};

const GENERAL_SUMMARY_PROMPT: &str = r#"请将以下对话历史压缩为结构化摘要。摘要必须足够详尽，使后续对话可以无缝继续，就像没有发生压缩一样。

重要原则：
- 按时间顺序分析每条消息，确保不遗漏任何关键信息
- 特别关注用户的明确反馈和修改指示
- 保留具体的文件名、代码片段、函数签名
- 记录遇到的错误及修复方法
- 如实记录所有用户消息（非工具结果类）

必须保留：
- 用户的原始需求和最终目标
- 关键决策和结论（包含原因）
- 当前进行中的工作状态
- 所有未完成的待办事项
- 重要的约束和规则
- 涉及的关键文件路径或资源标识
- 代码修改的具体内容

摘要格式（严格遵守）：
<context_summary>
## 用户目标
[详细描述用户的核心需求和意图]

## 关键技术概念
- [涉及的技术、框架、工具]

## 已完成
- [关键成果，包含具体文件和代码细节]

## 文件与代码
- [文件名]
  - [为什么重要]
  - [修改摘要]
  - [关键代码片段]

## 错误与修复
- [错误描述]：
  - [修复方法]
  - [用户反馈（如有）]

## 进行中
- [当前工作的精确状态，包含文件名和代码片段]

## 用户消息记录
- [列出所有非工具结果的用户消息要点]

## 关键决策
- [重要决定及其原因]

## 待办
- [待处理事项]

## 约束与规则
- [必须遵守的限制条件]

## 下一步
[基于最近工作的下一步计划，包含直接引用最近对话的内容以确保任务连续性]
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

/// Check the captured turn, never whichever request is currently newest.
pub(crate) fn ensure_summary_execution(
    validate: &(dyn Fn() -> Result<(), AppError> + Sync),
    token: Option<&tokio_util::sync::CancellationToken>,
) -> Result<(), AppError> {
    if token.is_some_and(|token| token.is_cancelled()) {
        return Err(AppError::UnknownError("执行已取消，未继续生成上下文摘要".into()));
    }
    validate()
}

async fn guarded_summary_request<T>(
    validate: &(dyn Fn() -> Result<(), AppError> + Sync),
    token: Option<&tokio_util::sync::CancellationToken>,
    request: impl std::future::Future<Output = Result<T, AppError>>,
) -> Result<T, AppError> {
    ensure_summary_execution(validate, token)?;
    let cancelled = async {
        match token {
            Some(token) => token.cancelled().await,
            None => std::future::pending::<()>().await,
        }
    };
    let result = tokio::select! {
        biased;
        _ = cancelled => return Err(AppError::UnknownError("执行已取消，未采用上下文摘要".into())),
        result = request => result?,
    };
    ensure_summary_execution(validate, token)?;
    Ok(result)
}

/// Generate a structured summary of the conversation body via LLM.
///
/// `body_messages` is the middle segment of the conversation (between system
/// prompt and the most recent tail messages) that needs to be compressed.
pub async fn generate_summary(
    client: &Client,
    model_name: &str,
    body_messages: &[MessageTuple],
    is_butler: bool,
    require_complete: bool,
    validate_execution: &(dyn Fn() -> Result<(), AppError> + Sync),
    cancel_token: Option<&tokio_util::sync::CancellationToken>,
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
    let response = guarded_summary_request(validate_execution, cancel_token, async {
        client.exec_chat(model_name, chat_request, None).await.map_err(|e| {
            AppError::UnknownError(format!("context compaction LLM call failed: {}", e))
        })
    }).await?;

    if require_complete {
        check_summary_stop_reason(response.stop_reason.as_ref())?;
    }
    let raw = response.first_text().unwrap_or("").to_string();
    let summary = validate_summary(&raw, is_butler, require_complete)?;

    debug!(summary_len = summary.len(), "context compaction summary generated");

    Ok(summary)
}

const SUMMARY_OPEN_TAG: &str = "<context_summary>";
const SUMMARY_CLOSE_TAG: &str = "</context_summary>";
const REQUIRED_SECTIONS: &[&str] = &["## 用户目标", "## 用户消息记录", "## 待办", "## 下一步"];
const BUTLER_REQUIRED_SECTIONS: &[&str] = &["## 子任务状态"];

/// 模型异常结束（被截断、被过滤、转去调用工具或其他非正常原因）时，摘要不能采用。
/// 未上报结束原因的适配器只依赖结构校验。
pub(crate) fn check_summary_stop_reason(stop_reason: Option<&StopReason>) -> Result<(), AppError> {
    match stop_reason {
        None | Some(StopReason::Completed(_)) | Some(StopReason::StopSequence(_)) => Ok(()),
        Some(reason) => Err(AppError::UnknownError(format!(
            "上下文摘要模型未正常结束（stop_reason={}），已保留原记录",
            reason.raw()
        ))),
    }
}

/// 校验摘要。`require_complete` 时（管家主会话、成员会话、任务会话）拒绝被截断或缺段的摘要，
/// 不再自动补标签——补上的结束标签会把半截摘要伪装成完整摘要。调用方失败时保留原记录。
pub(crate) fn validate_summary(
    raw: &str,
    is_butler: bool,
    require_complete: bool,
) -> Result<String, AppError> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(AppError::UnknownError("上下文摘要模型返回了空文本".into()));
    }
    if !require_complete {
        // 普通会话保持原有兼容行为
        return Ok(if trimmed.starts_with(SUMMARY_OPEN_TAG) {
            trimmed.to_owned()
        } else {
            format!("{SUMMARY_OPEN_TAG}\n{}\n{SUMMARY_CLOSE_TAG}", raw)
        });
    }
    let start = trimmed.find(SUMMARY_OPEN_TAG).ok_or_else(|| {
        AppError::UnknownError(format!("上下文摘要不完整：缺少开始标签 {SUMMARY_OPEN_TAG}，已保留原记录"))
    })?;
    let end = trimmed[start..].find(SUMMARY_CLOSE_TAG).map(|i| start + i).ok_or_else(|| {
        AppError::UnknownError(format!(
            "上下文摘要不完整：缺少结束标签 {SUMMARY_CLOSE_TAG}（摘要可能被截断，长度 {} 字节），已保留原记录",
            trimmed.len()
        ))
    })?;
    let summary = &trimmed[start..end + SUMMARY_CLOSE_TAG.len()];
    let missing: Vec<&str> = REQUIRED_SECTIONS
        .iter()
        .chain(if is_butler { BUTLER_REQUIRED_SECTIONS } else { &[] })
        .filter(|section| !summary.contains(**section))
        .copied()
        .collect();
    if !missing.is_empty() {
        return Err(AppError::UnknownError(format!(
            "上下文摘要不完整：缺少段落 {}，已保留原记录",
            missing.join("、")
        )));
    }
    Ok(summary.to_owned())
}

const MESSAGE_PREVIEW_LIMIT: usize = 8000;
const USER_HEAD_BYTES: usize = 5000;
const USER_TAIL_BYTES: usize = 3000;

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
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
        let truncated = if content.len() <= MESSAGE_PREVIEW_LIMIT {
            content.clone()
        } else if msg_type == "user" {
            // 用户长指令的末尾常是限制条件，保留开头与结尾
            let head_end = floor_char_boundary(content, USER_HEAD_BYTES);
            let tail_start = ceil_char_boundary(content, content.len() - USER_TAIL_BYTES);
            format!(
                "{}\n…（中间省略约 {} 字节，已保留开头与结尾）…\n{}",
                &content[..head_end],
                tail_start - head_end,
                &content[tail_start..]
            )
        } else {
            let end = floor_char_boundary(content, MESSAGE_PREVIEW_LIMIT);
            format!("{}...（内容过长，已截取前部分）", &content[..end])
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
        let msgs: Vec<MessageTuple> = vec![("response".into(), long_content, vec![])];
        let result = format_messages_for_summary(&msgs);
        assert!(result.contains("已截取前部分"));
        assert!(result.len() < 9000);
    }

    #[test]
    fn format_messages_keeps_tail_constraints_of_long_user_message() {
        let long_instruction = format!("{}最后注意：不要修改数据库结构", "需求说明。".repeat(1200));
        let msgs: Vec<MessageTuple> = vec![("user".into(), long_instruction, vec![])];
        let result = format_messages_for_summary(&msgs);
        assert!(result.contains("最后注意：不要修改数据库结构"));
        assert!(result.contains("已保留开头与结尾"));
        assert!(result.len() < 9000);
    }

    fn complete_summary(extra: &str) -> String {
        format!("<context_summary>\n## 用户目标\n写报告\n## 用户消息记录\n- 不要改库\n## 待办\n- 无\n## 下一步\n继续{extra}\n</context_summary>")
    }

    #[test]
    fn validate_summary_rejects_truncated_or_incomplete_for_butler_sessions() {
        let truncated = "<context_summary>\n## 用户目标\n写报告\n## 用户消息记";
        let err = validate_summary(truncated, false, true).unwrap_err().to_string();
        assert!(err.contains("结束标签"));
        assert!(validate_summary("## 用户目标 只有正文", false, true).is_err());
        let missing = "<context_summary>\n## 用户目标\nx\n</context_summary>";
        assert!(validate_summary(missing, false, true).unwrap_err().to_string().contains("用户消息记录"));
        // 总管家主会话还要求子任务状态
        assert!(validate_summary(&complete_summary(""), true, true).unwrap_err().to_string().contains("子任务状态"));
        let butler = complete_summary("\n## 子任务状态\n| 1 | 报告 | RUNNING | 写作 | - |");
        assert!(validate_summary(&butler, true, true).is_ok());
        // 标签外的多余文字被去掉
        let wrapped = format!("好的，摘要如下：\n{}\n以上。", complete_summary(""));
        assert_eq!(validate_summary(&wrapped, false, true).unwrap(), complete_summary(""));
    }

    #[test]
    fn summary_stop_reason_rejects_abnormal_endings() {
        assert!(check_summary_stop_reason(None).is_ok());
        assert!(check_summary_stop_reason(Some(&StopReason::from("stop".to_string()))).is_ok());
        for raw in ["length", "content_filter", "tool_calls", "error"] {
            let err = check_summary_stop_reason(Some(&StopReason::from(raw.to_string()))).unwrap_err();
            assert!(err.to_string().contains(raw), "{raw}");
        }
    }

    #[test]
    fn validate_summary_keeps_legacy_behavior_for_normal_chat() {
        assert!(validate_summary("   ", false, false).is_err());
        let wrapped = validate_summary("纯文本摘要", false, false).unwrap();
        assert!(wrapped.starts_with("<context_summary>") && wrapped.ends_with("</context_summary>"));
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
        assert!(GENERAL_SUMMARY_PROMPT.contains("文件与代码"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("错误与修复"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("用户消息记录"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("下一步"));
        assert!(GENERAL_SUMMARY_PROMPT.contains("<context_summary>"));
    }
}

#[cfg(test)]
#[path = "summary_execution_tests.rs"]
mod execution_tests;
