import { invoke } from "@tauri-apps/api/core";

import { MCPToolCall } from "@/data/MCPToolCall";
import type { PreviewCodeRequestEvent } from "@/utils/previewCode";

const MAX_TOOL_CALL_ENTRIES = 500;

const toolCallRequests = new Map<number, Promise<MCPToolCall>>();
// 待处理的预览请求会在没有事件通知的情况下被消费掉，只合并同一批卡片挂载时的并发读取，不缓存结果。
const previewListRequests = new Map<number, Promise<PreviewCodeRequestEvent[]>>();

function isTerminalToolStatus(status: string | undefined): boolean {
    return status === "success" || status === "failed" || status === "cancelled";
}

export function loadMcpToolCall(callId: number): Promise<MCPToolCall> {
    const existing = toolCallRequests.get(callId);
    if (existing) {
        toolCallRequests.delete(callId);
        toolCallRequests.set(callId, existing);
        return existing;
    }

    const request = invoke<MCPToolCall>("get_mcp_tool_call", { callId })
        .then((result) => {
            if (!isTerminalToolStatus(result.status) && toolCallRequests.get(callId) === request) {
                toolCallRequests.delete(callId);
            }
            return result;
        })
        .catch((error) => {
            if (toolCallRequests.get(callId) === request) {
                toolCallRequests.delete(callId);
            }
            throw error;
        });
    toolCallRequests.set(callId, request);
    if (toolCallRequests.size > MAX_TOOL_CALL_ENTRIES) {
        const oldest = toolCallRequests.keys().next().value;
        if (oldest !== undefined) {
            toolCallRequests.delete(oldest);
        }
    }
    return request;
}

export function invalidateMcpToolCall(callId: number) {
    toolCallRequests.delete(callId);
}

export function loadPreviewCodeRequests(conversationId: number): Promise<PreviewCodeRequestEvent[]> {
    const existing = previewListRequests.get(conversationId);
    if (existing) {
        return existing;
    }

    const request = invoke<PreviewCodeRequestEvent[]>("list_preview_code_requests_for_conversation", {
        conversationId,
        conversation_id: conversationId,
    }).finally(() => {
        if (previewListRequests.get(conversationId) === request) {
            previewListRequests.delete(conversationId);
        }
    });
    previewListRequests.set(conversationId, request);
    return request;
}
