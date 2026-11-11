import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import type { MCPToolCall } from "@/data/MCPToolCall";
import { useAskUserQuestion } from "./useInlineInteraction";

vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn() } }));

const QUESTIONS = [{
    header: "哑铃重量",
    question: "你家里的哑铃是什么重量规格？",
    multiSelect: false,
    options: [{ label: "2-5kg", description: "" }, { label: "6-10kg", description: "" }],
}];

// 应用重启后：原提问调用仍停在 executing，管家问题以稳定 ID 恢复出来。
const orphanedCall: MCPToolCall = {
    id: 506, conversation_id: 1522, message_id: 25575, server_id: 1, server_name: "UI交互工具",
    tool_name: "ask_user_question", parameters: JSON.stringify({ questions: QUESTIONS }),
    status: "executing", created_time: "2026-11-11 09:10:00",
};
const recovered = { request_id: "7565f194", conversation_id: 1522, questions: QUESTIONS };

describe("useAskUserQuestion submit", () => {
    beforeEach(() => {
        vi.mocked(toast.error).mockClear();
        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [orphanedCall]);
        mockInvokeHandler("list_recoverable_butler_questions", () => [recovered]);
    });
    afterEach(() => clearAllMockHandlers());

    it("should keep the questionnaire and show the error when the answer is not delivered", async () => {
        mockInvokeHandler("submit_ask_user_question_response", () => {
            throw "当前会话仍在运行，请稍后再回答恢复的问题";
        });
        const { result } = renderHook(() => useAskUserQuestion({ conversationId: 1522 }));
        await waitFor(() => expect(result.current.pendingRequest?.request_id).toBe("7565f194"));

        await act(() => result.current.handleSubmit("7565f194", { 哑铃重量: "2-5kg" }));

        expect(toast.error).toHaveBeenCalledWith("回答没有送达", {
            description: "当前会话仍在运行，请稍后再回答恢复的问题",
        });
        expect(result.current.pendingRequest?.request_id).toBe("7565f194");
        expect(result.current.viewMode).toBe("questionnaire");
        expect(result.current.readOnly).toBe(false);
    });

    it("should tell the user when a persisted questionnaire has no live request", async () => {
        mockInvokeHandler("list_recoverable_butler_questions", () => []);
        const submit = vi.fn();
        mockInvokeHandler("submit_ask_user_question_response", submit);
        const { result } = renderHook(() => useAskUserQuestion({ conversationId: 1522 }));
        await waitFor(() => expect(result.current.pendingRequest?.request_id).toBe("persisted-ask-506"));

        await act(() => result.current.handleSubmit("persisted-ask-506", { 哑铃重量: "2-5kg" }));

        expect(submit).not.toHaveBeenCalled();
        expect(toast.error).toHaveBeenCalledWith("回答没有送达", expect.objectContaining({
            description: expect.stringContaining("没有等待中的请求"),
        }));
    });
});
