import { parseStreamRetryMeta } from "@/utils/streamRetryAttempts";
import type { Message } from "@/data/Conversation";

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
        if (internalTurn || ["system", "reasoning", "tool_result", "error"].includes(message.message_type)) continue;
        if (message.message_type !== "response" && message.message_type !== "assistant") {
            result.push(message);
            continue;
        }
        if (message.content.trim() === BUTLER_SILENT_REPLY) continue;
        if (unpublished.has(message.id) || message.finish_time === null || parseStreamRetryMeta(message)?.status === "interrupted") continue;
        const content = /MCP_TOOL_CALL/i.test(message.content) ? message.content.replace(
            /(`+)[\s\S]*?\1|(~{3,})[\s\S]*?\2|<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\s*:[\s\S]*?(?:-->|$)|<mcp_tool_call(?:\s[^>]*)?>[\s\S]*?(?:<\/mcp_tool_call>|$)/gi,
            (segment, backticks, tildes) => {
                if (backticks || tildes) return segment;
                return "";
            },
        ) : message.content;
        const hasTool = content !== message.content
            || (!!message.tool_calls_json && !["[]", "null"].includes(message.tool_calls_json.trim()))
            || message.large_message_preview?.reason === "mcp_payload";
        if (!content.trim() && !message.attachment_list?.length) continue;
        if (hasTool) {
            if (message.attachment_list?.length) {
                result.push({ ...message, content: "", tool_calls_json: null, large_message_preview: null });
            }
        } else {
            result.push(message);
        }
    }
    return result;
}
