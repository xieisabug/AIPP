import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { conversationExportService } from "./conversationExportService";

vi.mock("@/components/conversation/ConversationExportRenderer", () => ({
    renderExportContent: vi.fn(), renderSingleMessageExportContent: vi.fn(),
}));

describe("conversation export data completeness", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        mockInvokeHandler("get_conversation_with_messages", () => ({
            id: 1, name: "test", messages: [{ id: 10, content: "answer" }],
        }));
    });

    it("should reject a tool record read failure rather than exporting incomplete data", async () => {
        const failure = new Error("tool records: database unavailable");
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => { throw failure; });
        await expect(conversationExportService.getExportData("1")).rejects.toBe(failure);
    });

    it("should allow a valid empty tool result and allow retry after a failed read", async () => {
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => { throw new Error("busy"); });
        await expect(conversationExportService.getExportData("1")).rejects.toThrow("busy");
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => []);
        await expect(conversationExportService.getExportData("1")).resolves.toMatchObject({ toolCalls: [] });
    });

    it("should retain tools belonging to exported messages when records load successfully", async () => {
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [
            { id: 5, message_id: 10 }, { id: 6, message_id: 20 },
        ]);
        const data = await conversationExportService.getExportData("1");
        expect(data.toolCalls).toEqual([{ id: 5, message_id: 10 }]);
    });
});
