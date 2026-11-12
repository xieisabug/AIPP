import { parseStreamRetryMeta } from "@/utils/streamRetryAttempts";
import type { Message } from "@/data/Conversation";
import { quoteExcerpt } from "./ButlerQuote";
import { findQuotedMessage, splitReplyQuote } from "./butlerReplyQuote";
import { visibleText } from "@/utils/turnContext";

// Kept in sync with BUTLER_SILENT_REPLY in src-tauri/src/api/butler_api.rs.
export const BUTLER_SILENT_REPLY = "[[NO_REPLY]]";

// Rendering only. Conversation history, runtime and export retain the original messages.
export function butlerTranscript(messages: Message[], unpublished = new Set<number>()): Message[] {
    const result: Message[] = [];
    let internalTurn = false;
    for (const message of messages) {
        if (message.message_type === "user") {
            internalTurn = false;
            if (message.metadata_json) {
                try {
                    const metadata = JSON.parse(message.metadata_json);
                    if (metadata?.origin === "butler_peer") {
                        // audience=user: the assistant that took the work answers the user here under its own
                        // name (delegated work, or continuing it after a peer replied). audience=peer: the
                        // answer only goes back to another assistant. Older deliveries carry no audience:
                        // requests were internal, replies/receipts led to a summary for the human.
                        internalTurn = metadata.audience
                            ? metadata.audience !== "user"
                            : metadata.leg !== "reply" && metadata.leg !== "receipt";
                        continue;
                    }
                    if (metadata?.origin === "internal") continue;
                } catch { /* Historical metadata can be absent or invalid. */ }
            }
        }
        if (internalTurn || ["system", "reasoning", "tool_result"].includes(message.message_type)) continue;
        // 失败和 ChatUI 一样显示成可展开的一行；之后已有回复的旧错误属于重试过程，简洁聊天不再显示。
        if (message.message_type === "error") {
            if (!unpublished.has(message.id) && !message.isSupersededError) result.push(message);
            continue;
        }
        if (message.message_type !== "response" && message.message_type !== "assistant") {
            result.push(message);
            continue;
        }
        if (message.content.trim() === BUTLER_SILENT_REPLY) continue;
        if (unpublished.has(message.id) || message.finish_time === null || parseStreamRetryMeta(message)?.status === "interrupted") continue;
        // preview_message 展示的是另一条回复，本身就是交给用户的结果，不是执行过程：简洁聊天里保留这张卡片。
        const shownPreviews: string[] = [];
        const content = /MCP_TOOL_CALL/i.test(message.content) ? message.content.replace(
            /(`+)[\s\S]*?\1|(~{3,})[\s\S]*?\2|<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\s*:[\s\S]*?(?:-->|$)|<mcp_tool_call(?:\s[^>]*)?>[\s\S]*?(?:<\/mcp_tool_call>|$)/gi,
            (segment, backticks, tildes) => {
                if (backticks || tildes) return segment;
                if (isCompletePreviewMessageCall(segment)) shownPreviews.push(segment);
                return "";
            },
        ) : message.content;
        const hasTool = content !== message.content
            || (!!message.tool_calls_json && !["[]", "null"].includes(message.tool_calls_json.trim()))
            || message.large_message_preview?.reason === "mcp_payload";
        if (!content.trim() && !message.attachment_list?.length && !shownPreviews.length) continue;
        if (hasTool) {
            if (shownPreviews.length) {
                // 只留引用卡片：同一条里的过程说明与其他工具调用仍不显示。
                result.push({ ...message, content: shownPreviews.join("\n\n"), large_message_preview: null });
            } else if (message.attachment_list?.length) {
                result.push({ ...message, content: "", tool_calls_json: null, large_message_preview: null });
            }
        } else {
            result.push(withReplyQuote(message, result));
        }
    }
    return result;
}

const PREVIEW_MESSAGE_TOOL = /"tool_name"\s*:\s*"preview_message"/;

/** 已完整写出的 preview_message 调用标记（流式中的半截标记不算）。 */
function isCompletePreviewMessageCall(segment: string): boolean {
    return /^<!--\s*MCP_TOOL_CALL\s*:/i.test(segment) && segment.endsWith("-->") && PREVIEW_MESSAGE_TOOL.test(segment);
}

/** 回复开头的 `[[QUOTE:原话]]` 显示成引用标签（指向已显示的那条消息），正文里不再出现标记。 */
function withReplyQuote(message: Message, earlier: Message[]): Message {
    const parts = splitReplyQuote(message.content);
    if (!parts) return message;
    const quoted = findQuotedMessage(parts.fragment, earlier);
    const reply_quote = quoted && {
        label: `${quoted.message_type === "user" ? "我：" : ""}${quoteExcerpt(visibleText(quoted.content), 40)}`,
        target: quoted.id,
    };
    return { ...message, content: parts.body, reply_quote };
}
