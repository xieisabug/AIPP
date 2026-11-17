import { act, renderHook, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import type { MCPToolCall } from "@/data/MCPToolCall";
import type { PreviewFileRequest } from "@/components/InlineInteractionCards";
import { usePreviewFile } from "./useInlineInteraction";

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason: Error) => void;
    const promise = new Promise<T>((done, fail) => { resolve = done; reject = fail; });
    return { promise, resolve, reject };
}
function call(conversationId: number): MCPToolCall {
    return { id: conversationId, conversation_id: conversationId, message_id: conversationId * 10,
        server_id: 1, server_name: "UI", tool_name: "preview_file", status: "success",
        parameters: JSON.stringify({ files: [{ title: `file-${conversationId}`, type: "text", content: "hello" }] }),
        created_time: "2026-11-16 12:00:00" };
}
function request(conversationId?: number): PreviewFileRequest {
    return { request_id: "live", conversation_id: conversationId,
        files: [{ title: "live-file", type: "text", content: "hello" }] };
}

describe("usePreviewFile conversation isolation", () => {
    beforeEach(() => {
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
        mockInvokeHandler("prepare_preview_file_request_for_ui", args => args?.request);
    });
    afterEach(() => { clearAllMockHandlers(); vi.useRealTimers(); });

    it("should hide the previous preview immediately when switching to a loading conversation", async () => {
        const next = deferred<MCPToolCall[]>();
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", args => args?.conversationId === 1 ? [call(1)] : next.promise);
        const { result, rerender } = renderHook(({ id }) => usePreviewFile({ conversationId: id }), { initialProps: { id: 1 } });
        await waitFor(() => expect(result.current.callId).toBe(1));
        rerender({ id: 2 });
        expect(result.current.pendingRequest).toBeNull();
        expect(result.current.managedCallId).toBeUndefined();
        await act(async () => next.resolve([]));
    });

    it.each(["query", "prepare", "prepare-error"])("should ignore a late %s result when the conversation changes", async stage => {
        const oldQuery = deferred<MCPToolCall[]>();
        const oldPrepare = deferred<PreviewFileRequest>();
        const preparing = vi.fn((args?: Record<string, unknown>) => args?.conversationId === 1 ? oldPrepare.promise : args?.request);
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", args => args?.conversationId === 1
            ? (stage === "query" ? oldQuery.promise : [call(1)]) : [call(2)]);
        mockInvokeHandler("prepare_preview_file_request_for_ui", preparing);
        const { result, rerender } = renderHook(({ id }) => usePreviewFile({ conversationId: id }), { initialProps: { id: 1 } });
        if (stage !== "query") await waitFor(() => expect(preparing).toHaveBeenCalled());
        rerender({ id: 2 });
        await waitFor(() => expect(result.current.callId).toBe(2));
        await act(async () => {
            if (stage === "prepare-error") {
                oldPrepare.reject(new Error("old preview preparation failed"));
            } else {
                oldQuery.resolve([call(1)]);
                oldPrepare.resolve(request(1));
            }
        });
        expect(result.current.callId).toBe(2);
        expect(result.current.pendingRequest?.conversation_id).toBe(2);
    });

    it("should reject unowned and foreign events when a conversation is displayed", async () => {
        const { result } = renderHook(() => usePreviewFile({ conversationId: 2 }));
        await act(async () => {
            await emit("preview-file-request", request());
            await emit("preview-file-request", request(1));
        });
        expect(result.current.pendingRequest).toBeNull();
        await act(async () => { await emit("preview-file-request", request(2)); });
        expect(result.current.pendingRequest?.conversation_id).toBe(2);
    });

    it("should reject all events when no conversation is displayed", async () => {
        const { result } = renderHook(() => usePreviewFile());
        await act(async () => { await emit("preview-file-request", request(1)); });
        expect(result.current.pendingRequest).toBeNull();
    });

    it("should cancel delayed refreshes when switching conversations", async () => {
        vi.useFakeTimers();
        const query = vi.fn(() => []);
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", query);
        const { rerender } = renderHook(({ id }) => usePreviewFile({ conversationId: id }), { initialProps: { id: 1 } });
        await act(async () => { await emit("preview-file-request", request(1)); });
        rerender({ id: 2 });
        query.mockClear();
        await act(async () => { await vi.advanceTimersByTimeAsync(300); });
        expect(query).not.toHaveBeenCalled();
    });
});

it("should use the event's own call and message even when a different preview is executing", async () => {
    const other = { ...call(2), status: "executing" };
    mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [other]);
    mockInvokeHandler("prepare_preview_file_request_for_ui", args => args?.request);
    try {
        const { result } = renderHook(() => usePreviewFile({ conversationId: 2 }));
        await waitFor(() => expect(result.current.callId).toBe(2));
        await act(async () => { await emit("preview-file-request", { ...request(2), call_id: 17, message_id: 19 }); });
        expect([result.current.callId, result.current.messageId]).toEqual([17, 19]);
        await act(async () => { await emit("preview-file-request", { ...request(2), request_id: "no-match" }); });
        expect(result.current.callId).toBeNull();
        expect(result.current.managedCallId).toBeUndefined();
    } finally {
        clearAllMockHandlers();
    }
});
