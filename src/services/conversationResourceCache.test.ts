import { afterEach, describe, expect, it, vi } from "vitest";

import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import {
    invalidateMcpToolCall,
    loadMcpToolCall,
    loadPreviewCodeRequests,
} from "./conversationResourceCache";

function countCalls(command: string): number {
    return vi.mocked(invoke).mock.calls.filter(([name]) => name === command).length;
}

describe("conversationResourceCache", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("should reuse a finished tool call until it is invalidated", async () => {
        let status = "failed";
        mockInvokeHandler("get_mcp_tool_call", () => ({ id: 9001, status }));

        expect((await loadMcpToolCall(9001)).status).toBe("failed");
        expect((await loadMcpToolCall(9001)).status).toBe("failed");
        expect(countCalls("get_mcp_tool_call")).toBe(1);

        status = "success";
        invalidateMcpToolCall(9001);

        expect((await loadMcpToolCall(9001)).status).toBe("success");
        expect(countCalls("get_mcp_tool_call")).toBe(2);
    });

    it("should refetch a tool call that is still executing", async () => {
        mockInvokeHandler("get_mcp_tool_call", () => ({ id: 9002, status: "executing" }));

        await loadMcpToolCall(9002);
        await loadMcpToolCall(9002);

        expect(countCalls("get_mcp_tool_call")).toBe(2);
    });

    it("should share one in-flight preview list request but not keep its result", async () => {
        let requests = [{ request_id: "first" }];
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => requests);

        const [first, second] = await Promise.all([
            loadPreviewCodeRequests(77),
            loadPreviewCodeRequests(77),
        ]);
        expect(first).toBe(second);
        expect(countCalls("list_preview_code_requests_for_conversation")).toBe(1);

        requests = [{ request_id: "second" }];
        const next = await loadPreviewCodeRequests(77);

        expect(next[0].request_id).toBe("second");
        expect(countCalls("list_preview_code_requests_for_conversation")).toBe(2);
    });
});
