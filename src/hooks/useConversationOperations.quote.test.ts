import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useConversationOperations } from "./useConversationOperations";

vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => false }));
const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));
vi.mock("./useConversationManager", () => ({
    default: () => ({ deleteConversation: vi.fn(), updateConversationName: vi.fn() }),
}));

function setup(aiIsResponsing: boolean, overrides: Record<string, unknown> = {}) {
    const props: any = {
        conversation: { id: 20, assistant_id: 2, conversation_kind: "butler_contact" },
        selectedAssistant: 2,
        assistants: [{ id: 2, name: "写作", assistant_type: 0 }],
        setMessages: vi.fn(), inputText: "再改短一点", setInputText: vi.fn(), fileInfoList: [], clearFileInfoList: vi.fn(),
        aiIsResponsing, setAiIsResponsing: vi.fn(), onChangeConversationId: vi.fn(), setShiningMessageIds: vi.fn(),
        setManualShineMessage: vi.fn(), updateShiningMessages: vi.fn(), clearShiningMessages: vi.fn(),
        assistantTypePluginMap: new Map(), assistantRunApi: {},
        quote: { kind: "task", id: 41 }, clearQuote: vi.fn(),
        busySendBehavior: "interrupt",
        ...overrides,
    };
    const { result } = renderHook(() => useConversationOperations(props));
    return { result, props };
}

describe("useConversationOperations quote reply", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        toastMock.error.mockClear();
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
    it.each(["谢谢", "为什么这样做？", "报告改成演示稿", "先停一下"])("should leave meaning of %s to the assistant without an implicit goal intent", async (prompt) => {
        mockInvokeHandler("ask_ai", () => ({ conversation_id: 20, request_prompt_result_with_context: "" }));
        const { result } = setup(false, { inputText: prompt, quote: null });
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("ask_ai", expect.objectContaining({
            request: expect.objectContaining({ prompt }),
        })));
        const request = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "ask_ai")?.[1]?.request;
        expect(request).not.toHaveProperty("butler_intent");
        expect(invoke).not.toHaveBeenCalledWith("cancel_ai", expect.anything());
    });

    it("should preserve interrupt delivery and quote identity without assigning a message purpose", async () => {
        mockInvokeHandler("enqueue_conversation_message", () => ({}));
        const { result } = setup(true);
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("enqueue_conversation_message", {
            request: expect.objectContaining({ prompt: "再改短一点", quote: { kind: "task", id: 41 } }),
            queueKind: "interrupt",
        }));
        const request = vi.mocked(invoke).mock.calls.find(([cmd]) => cmd === "enqueue_conversation_message")?.[1]?.request;
        expect(request).not.toHaveProperty("butler_intent");
        expect(invoke).not.toHaveBeenCalledWith("ask_ai", expect.anything());
    });

    it("should retain ordinary queue behavior outside the Butler interrupt configuration", async () => {
        mockInvokeHandler("enqueue_conversation_message", () => ({}));
        const { result } = setup(true, { busySendBehavior: "queue" });
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("enqueue_conversation_message", expect.objectContaining({ queueKind: "normal" })));
    });

    it("should keep text, attachments and quote when the native backend rejects unsupported interruption", async () => {
        mockInvokeHandler("enqueue_conversation_message", () => { throw new Error("当前原生 Agent 不支持运行中插队，请停止后发送"); });
        const { result, props } = setup(true, {
            assistants: [{ id: 2, name: "Claude", assistant_type: 4 }],
            fileInfoList: [{ id: 10 }],
        });
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("不支持运行中插队")));
        expect(props.setInputText).not.toHaveBeenCalled();
        expect(props.clearFileInfoList).not.toHaveBeenCalled();
        expect(props.clearQuote).not.toHaveBeenCalled();
        expect(invoke).not.toHaveBeenCalledWith("ask_ai", expect.anything());
    });

    it("should keep the existing stop action independent of model interpretation", async () => {
        mockInvokeHandler("cancel_ai", () => ({}));
        const { result, props } = setup(true, { inputText: "" });
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("cancel_ai", { conversationId: 20 }));
        expect(invoke).not.toHaveBeenCalledWith("enqueue_conversation_message", expect.anything());
        expect(invoke).not.toHaveBeenCalledWith("ask_ai", expect.anything());
        expect(props.clearShiningMessages).toHaveBeenCalled();
    });
});
