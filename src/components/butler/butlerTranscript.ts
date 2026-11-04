import type { Message } from "@/data/Conversation";

const deliveryTools = new Set(["preview_code", "preview_file", "show_artifact"]);

function isDeliveryMarker(segment: string): boolean {
    if (segment.startsWith("<!--")) {
        try {
            const marker = JSON.parse(segment.slice(segment.indexOf(":") + 1, -3));
            return deliveryTools.has(marker.tool_name);
        } catch { return false; }
    }
    return deliveryTools.has(segment.match(/<tool_name>\s*([^<]+?)\s*<\/tool_name>/i)?.[1] ?? "");
}

// Rendering only. Conversation history, runtime and export retain the original messages.
export function butlerTranscript(messages: Message[], needsAttention = new Set<number>()): Message[] {
    const result: Message[] = [];
    let toolSteps: number[] = [];
    const superseded = new Set<number>();
    for (const message of messages) {
        if (message.message_type === "user") {
            if (message.metadata_json) {
                try {
                    if (JSON.parse(message.metadata_json)?.origin === "internal") continue;
                } catch { /* Historical metadata can be absent or invalid. */ }
            }
            toolSteps = [];
        }
        if (["system", "reasoning", "tool_result"].includes(message.message_type)) continue;
        if (message.message_type !== "response" && message.message_type !== "assistant") {
            result.push(message);
            continue;
        }
        if (needsAttention.has(message.id)) {
            result.push(message);
            continue;
        }
        let hasDelivery = false;
        const content = /MCP_TOOL_CALL/i.test(message.content) ? message.content.replace(
            /(`+)[\s\S]*?\1|(~{3,})[\s\S]*?\2|<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\s*:[\s\S]*?(?:-->|$)|<mcp_tool_call(?:\s[^>]*)?>[\s\S]*?(?:<\/mcp_tool_call>|$)/gi,
            (segment, backticks, tildes) => {
                if (backticks || tildes) return segment;
                if (isDeliveryMarker(segment)) {
                    hasDelivery = true;
                    return segment;
                }
                return "";
            },
        ) : message.content;
        const hasTool = hasDelivery || content !== message.content
            || (!!message.tool_calls_json && !["[]", "null"].includes(message.tool_calls_json.trim()))
            || message.large_message_preview?.reason === "mcp_payload";
        if (!content.trim() && !message.attachment_list?.length) continue;
        if (hasTool) {
            if (!hasDelivery && !message.attachment_list?.length) toolSteps.push(message.id);
            result.push({ ...message, content, tool_calls_json: null, large_message_preview: null });
        } else {
            // Once the answer arrives, intermediate tool narration leaves this turn.
            for (const id of toolSteps) superseded.add(id);
            toolSteps = [];
            result.push(message);
        }
    }
    return superseded.size ? result.filter(message => !superseded.has(message.id)) : result;
}
