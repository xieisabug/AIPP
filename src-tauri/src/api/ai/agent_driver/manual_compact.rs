//! 手动压缩上下文：用户发送 `/compact`（或点详情面板的“压缩上下文”）时，
//! 支持的原生 Agent 走各自的压缩通道，而不是把它当普通文本交给模型：
//! - Claude Code：原样作为用户消息发送，由 CLI 自带的 `/compact` 命令处理（`compact_boundary` 帧表示完成）；
//! - Codex：改发 app-server 的 `thread/compact/start`，进度仍以 `turn/*`、`item/*` 通知到达。
//! 压缩本身不产生正文，完成后由驱动写入一条说明，避免被当成“未返回任何内容”的失败。

pub(crate) const MANUAL_COMPACT_COMMAND: &str = "/compact";

/// 本轮是否为手动压缩命令（忽略首尾空白与系统附加的本轮上下文块）。
pub(crate) fn is_manual_compact_prompt(prompt: &str) -> bool {
    crate::utils::turn_context::visible(prompt).trim() == MANUAL_COMPACT_COMMAND
}

/// 压缩完成后写入回复的说明文字。
pub(crate) fn compact_done_text(pre_tokens: Option<u64>) -> String {
    match pre_tokens {
        Some(tokens) if tokens > 0 => format!("上下文已压缩（压缩前约 {tokens} tokens）。"),
        _ => "上下文已压缩。".to_string(),
    }
}

#[cfg(test)]
#[path = "manual_compact_tests.rs"]
mod tests;
