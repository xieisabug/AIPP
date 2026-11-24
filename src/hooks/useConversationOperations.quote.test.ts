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

    it("should publish the request before IPC completes and preserve newer input on rejection", async () => {
        let reject!: (error: Error) => void;
        mockInvokeHandler("ask_ai", () => new Promise((_resolve, fail) => { reject = fail; }));
        let draft = "再改短一点";
        const onComposerMessage = vi.fn();
        const onComposerResult = vi.fn();
        const { result, props } = setup(false, { onComposerMessage, onComposerResult,
            setInputText: (value: string | ((text: string) => string)) => { draft = typeof value === "function" ? value(draft) : value; },
        });
        act(() => { result.current.handleSend(); });
        const row = onComposerMessage.mock.calls[0][0];
        expect(row).toMatchObject({content: "再改短一点", sendStatus: "sending", conversation_id: 20});
        expect(invoke).toHaveBeenCalledWith("ask_ai", expect.objectContaining({
            request: expect.objectContaining({client_request_id: row.clientRequestId}),
        }));
        expect(draft).toBe("");
        draft = "后写的草稿";
        await act(async () => reject(new Error("插件拒绝")));
        expect(draft).toBe("后写的草稿");
        expect(onComposerResult).toHaveBeenCalledWith(row.clientRequestId, { conversationId: 20, error: "插件拒绝" });
        expect(props.clearQuote).not.toHaveBeenCalled();
    });

    it("should use the queue receipt when its notification has not arrived", async () => {
        let confirm!: (value: unknown) => void;
        mockInvokeHandler("enqueue_conversation_message", () => new Promise(resolve => { confirm = resolve; }));
        const onComposerMessage = vi.fn();
        const onQueuedAccepted = vi.fn();
        const { result } = setup(true, { onComposerMessage, onQueuedAccepted, onComposerResult: () => false });
        act(() => { result.current.handleSend(); });
        const row = onComposerMessage.mock.calls[0][0];
        const queued = { id: 7, conversation_id: 20, request_json: JSON.stringify({client_request_id: row.clientRequestId}) };
        expect(invoke).toHaveBeenCalledWith("enqueue_conversation_message", expect.objectContaining({
            request: expect.objectContaining({client_request_id: row.clientRequestId}), queueKind: "interrupt",
        }));
        await act(async () => confirm(queued));
        expect(onQueuedAccepted).toHaveBeenCalledWith(queued);
    });

    it("should send the continuation through the current conversation with its attachments", async () => {
        mockInvokeHandler("ask_ai", () => ({ conversation_id: 20 }));
        const { result } = setup(false, { inputText: " ", fileInfoList: [{ id: 12 }] });
        await act(async () => { result.current.handleSend("继续你的工作"); });
        expect(invoke).toHaveBeenCalledWith("ask_ai", expect.objectContaining({ request: expect.objectContaining({
            conversation_id: "20", assistant_id: 2, attachment_list: [12], prompt: "继续你的工作",
        }) }));
    });

    it("should move attachments into the pending bubble on send and return them when the send is rejected", async () => {
        let reject!: (error: Error) => void;
        mockInvokeHandler("ask_ai", () => new Promise((_resolve, fail) => { reject = fail; }));
        const files = [
            { id: 31, name: "a.png", path: "C:/files/a.png", type: 1, thumbnail: "data:image/png;base64,AA" },
            { id: 32, name: "b.docx", path: "C:/files/b.docx", type: 4 },
        ];
        const onComposerMessage = vi.fn();
        const returnFileInfoList = vi.fn();
        const { result, props } = setup(false, {
            onComposerMessage, onComposerResult: vi.fn(), fileInfoList: files, returnFileInfoList, quote: null,
        });
        act(() => { result.current.handleSend(); });
        // The backend extracts document text before it saves the row; the bubble and composer must not wait for it.
        expect(onComposerMessage.mock.calls[0][0].attachment_list).toEqual([
            { id: 31, attachment_type: "Image", attachment_url: "C:/files/a.png", attachment_content: "data:image/png;base64,AA" },
            { id: 32, attachment_type: "Word", attachment_url: "C:/files/b.docx", attachment_content: undefined },
        ]);
        expect(props.clearFileInfoList).toHaveBeenCalledTimes(1);
        expect(returnFileInfoList).not.toHaveBeenCalled();
        await act(async () => reject(new Error("附件读取失败")));
        expect(returnFileInfoList).toHaveBeenCalledWith(files);
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
        mockInvokeHandler("enqueue_conversation_message", () => { throw { ProviderError: "目标回合已结束" }; });
        const { result, props } = setup(true, {
            assistants: [{ id: 2, name: "Claude", assistant_type: 4 }],
            fileInfoList: [{ id: 10 }],
        });
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("目标回合已结束")));
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
