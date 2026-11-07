import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useConversationOperations } from "./useConversationOperations";

vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => false }));
vi.mock("sonner", () => ({ toast: { error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() } }));
vi.mock("./useConversationManager", () => ({
    default: () => ({ deleteConversation: vi.fn(), updateConversationName: vi.fn() }),
}));

function setup(aiIsResponsing: boolean, butlerIntent?: "supplement" | "replace" | "aside" | "stop") {
    const props: any = {
        conversation: { id: 20, assistant_id: 2 },
        selectedAssistant: 2,
        assistants: [{ id: 2, name: "写作", assistant_type: 0 }],
        setMessages: vi.fn(), inputText: "再改短一点", setInputText: vi.fn(), fileInfoList: [], clearFileInfoList: vi.fn(),
        butlerIntent, aiIsResponsing, setAiIsResponsing: vi.fn(), onChangeConversationId: vi.fn(), setShiningMessageIds: vi.fn(),
        setManualShineMessage: vi.fn(), updateShiningMessages: vi.fn(), clearShiningMessages: vi.fn(),
        assistantTypePluginMap: new Map(), assistantRunApi: {},
        quote: { kind: "task", id: 41 }, clearQuote: vi.fn(),
    };
    const { result } = renderHook(() => useConversationOperations(props));
    return { result, props };
}

describe("useConversationOperations quote reply", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_assistant_execution_capability", () => ({ available: true, issues: [] }));
        vi.mocked(invoke).mockClear();
    });

    it("should send the stable quote identity with the message and clear the quote", async () => {
        mockInvokeHandler("ask_ai", () => ({ conversation_id: 20, request_prompt_result_with_context: "" }));
        const { result, props } = setup(false);
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("ask_ai", expect.objectContaining({
            request: expect.objectContaining({ prompt: "再改短一点", quote: { kind: "task", id: 41 } }),
        })));
        expect(props.clearQuote).toHaveBeenCalled();
    });

    it("should keep the quote on a message queued while the assistant is still running", async () => {
        mockInvokeHandler("enqueue_conversation_message", () => ({}));
        const { result, props } = setup(true);
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(vi.mocked(invoke)).toHaveBeenCalledWith("enqueue_conversation_message", expect.objectContaining({
            request: expect.objectContaining({ quote: { kind: "task", id: 41 } }),
        })));
        await waitFor(() => expect(props.clearQuote).toHaveBeenCalled());
    });
    it.each(["replace", "supplement", "aside"] as const)("keeps explicit %s intent in the queued request", async (intent) => {
        mockInvokeHandler("enqueue_conversation_message", () => ({}));
        const { result } = setup(true, intent);
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("enqueue_conversation_message", expect.objectContaining({
            request: expect.objectContaining({ butler_intent: intent }),
        })));
    });
    it("persists stop intent before cancelling and does not invoke a model", async () => {
        mockInvokeHandler("enqueue_conversation_message", () => ({}));
        mockInvokeHandler("cancel_ai", () => ({}));
        const { result } = setup(true, "stop");
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("cancel_ai", { conversationId: 20 }));
        const commands=vi.mocked(invoke).mock.calls.map(([command]) => command);
        expect(commands.indexOf("enqueue_conversation_message")).toBeLessThan(commands.indexOf("cancel_ai"));
        expect(commands).not.toContain("ask_ai");
    });

});
