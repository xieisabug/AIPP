import { parseStreamRetryMeta } from "@/utils/streamRetryAttempts";
import { readScheduledTaskMetadata, type Message } from "@/data/Conversation";
import { isButlerInteractionTool } from "./ButlerPresentationContext";
import { quoteExcerpt } from "./ButlerQuote";
import { findQuotedMessage, splitReplyQuote } from "./butlerReplyQuote";
import { visibleText } from "@/utils/turnContext";

// Kept in sync with BUTLER_SILENT_REPLY in src-tauri/src/api/butler_api.rs.
export const BUTLER_SILENT_REPLY = "[[NO_REPLY]]";

// Rendering only. Conversation history, runtime and export retain the original messages.
export function butlerTranscript(messages: Message[], unpublished = new Set<number>(), questionIds = new Set<string>(),
    calls?: ReadonlyMap<number, { tool_name?: string }>): Message[] {
    const result: Message[] = [];
    let internalTurn = false;
    for (const message of messages) {
        if (message.message_type === "user") {
            internalTurn = false;
            // Older continuations predate origin metadata. Match the generated envelope and a real question in this conversation.
            const recovery = /^\[用户回答已恢复的问题，问题编号 ([\w-]+)\]\n问题：/.exec(message.content);
            if (recovery && questionIds.has(recovery[1]) && visibleText(message.content).trimEnd().endsWith("旧权限不自动恢复。")) continue;
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
                    // 定时任务结果前的触发消息不是用户发言；结果本身带「来自定时任务」标签
                    if (metadata?.origin === "scheduled_task") continue;
                } catch { /* Historical metadata can be absent or invalid. */ }
            }
        }
        const scheduled = message.message_type === "response" ? readScheduledTaskMetadata(message.metadata_json) : null;
        if (scheduled) internalTurn = false;
        const scheduledResult = scheduled?.role === "result";
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
        if (parseStreamRetryMeta(message)?.status === "interrupted") continue;
        const pending = (!scheduledResult && unpublished.has(message.id)) || message.finish_time === null;
        // 交互卡可以在当前轮结束前显示，正文仍等到正式交付。
        const shownPreviews: string[] = [];
        const content = /MCP_TOOL_CALL/i.test(message.content) ? message.content.replace(
            /(`+)[\s\S]*?\1|(~{3,})[\s\S]*?\2|<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\s*:(?:"(?:\\.|[^"\\])*(?:"|\\?$)|[^"])*?(?:-->|$)|<mcp_tool_call(?:\s[^>]*)?>[\s\S]*?(?:<\/mcp_tool_call>|$)/gi,
            (segment, backticks, tildes) => {
                if (backticks || tildes) return segment;
                const name = completeToolName(segment, calls);
                if (isButlerInteractionTool(name) || (!pending && name === "preview_message" && !segment.startsWith("<!-- MCP_TOOL_CALL_STREAMING"))) {
                    if (scheduledResult && !pending) return segment;
                    shownPreviews.push(segment);
                }
                return "";
            },
        ) : message.content;
        if (pending && !shownPreviews.length) continue;
        const hasTool = content !== message.content
            || (!!message.tool_calls_json && !["[]", "null"].includes(message.tool_calls_json.trim()))
            || message.large_message_preview?.reason === "mcp_payload";
        if (!content.trim() && !message.attachment_list?.length && !shownPreviews.length) continue;
        if (scheduledResult && !pending && hasTool) {
            result.push(withReplyQuote({ ...message, content: content.trim(), tool_calls_json: null, large_message_preview: null }, result));
        } else if (hasTool) {
            if (shownPreviews.length) {
                // 只留面向用户的卡片：同一条里的过程说明与其他工具调用仍不显示。
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

function completeToolName(segment: string, calls?: ReadonlyMap<number, { tool_name?: string }>): string | undefined {
    const comment = segment.match(/^<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\s*:([\s\S]*)-->$/i);
    if (comment) {
        try {
            const marker = JSON.parse(comment[1]);
            return marker?.display_tool_name ?? marker?.tool_name ?? calls?.get(Number(marker?.call_id ?? marker))?.tool_name;
        } catch {
            return undefined;
        }
    }
    if (segment.endsWith("</mcp_tool_call>")) {
        return segment.match(/<tool_name>\s*([^<]+?)\s*<\/tool_name>/i)?.[1];
    }
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
