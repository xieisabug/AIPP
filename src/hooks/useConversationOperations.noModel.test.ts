import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { NO_MODEL_CONFIGURED_MESSAGE } from "@/lib/askAiError";
import { useConversationOperations } from "./useConversationOperations";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => platformState.mobile }));
const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));
vi.mock("./useConversationManager", () => ({
    default: () => ({ deleteConversation: vi.fn(), updateConversationName: vi.fn() }),
}));

function setup(quietRuntimeErrors = false, claudePermissionMode = "", overrides = {}) {
    const props: any = {
        quietRuntimeErrors,
        claudePermissionMode,
        conversation: null,
        selectedAssistant: 1,
        assistants: [{ id: 1, name: "快速使用助手", assistant_type: 0 }],
        onMessageSaved: vi.fn(),
        inputText: "你好",
        setInputText: vi.fn(),
        fileInfoList: [],
        clearFileInfoList: vi.fn(),
        aiIsResponsing: false,
        setAiIsResponsing: vi.fn(),
        onChangeConversationId: vi.fn(),
        setShiningMessageIds: vi.fn(),
        setManualShineMessage: vi.fn(),
        updateShiningMessages: vi.fn(),
        clearShiningMessages: vi.fn(),
        assistantTypePluginMap: new Map(),
        assistantRunApi: {},
        ...overrides,
    };
    const { result } = renderHook(() => useConversationOperations(props));
    return { result, props };
}

describe.each([
    ["desktop", false],
    ["mobile", true],
])("useConversationOperations without a configured model (%s)", (_label, mobile) => {
    beforeEach(() => {
        platformState.mobile = mobile;
        clearAllMockHandlers();
        mockInvokeHandler("get_assistant_execution_capability", () => ({ available: true, issues: [] }));
        vi.mocked(invoke).mockClear();
        toastMock.error.mockClear();
    });

    it("switches to the new conversation immediately and shows the no-model error", async () => {
        mockInvokeHandler("ask_ai", () => ({
            conversation_id: 42,
            request_prompt_result_with_context: "你好",
            error_message: NO_MODEL_CONFIGURED_MESSAGE,
        }));
        const { result, props } = setup();
        await act(async () => {
            result.current.handleSend();
        });
        await waitFor(() => expect(props.onChangeConversationId).toHaveBeenCalledWith("42"));
        expect(props.setAiIsResponsing).toHaveBeenLastCalledWith(false);
        expect(props.clearShiningMessages).toHaveBeenCalled();
        expect(toastMock.error).toHaveBeenCalledWith("快速使用助手 运行失败", {
            description: NO_MODEL_CONFIGURED_MESSAGE,
        });
        // 不能静默换模型：前端没有选择覆盖模型时不下发 override_model_id
        expect(vi.mocked(invoke)).toHaveBeenCalledWith(
            "ask_ai",
            expect.objectContaining({
                request: expect.objectContaining({ override_model_id: undefined, assistant_id: 1 }),
                overrideModelConfig: undefined,
            }),
        );
    });

    it("should reset runtime state without a technical toast when compact mode is enabled", async () => {
        mockInvokeHandler("ask_ai", () => ({
            conversation_id: 42, request_prompt_result_with_context: "", error_message: NO_MODEL_CONFIGURED_MESSAGE,
        }));
        const { result, props } = setup(true);
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(props.setAiIsResponsing).toHaveBeenLastCalledWith(false));
        expect(props.onChangeConversationId).toHaveBeenCalledWith("42");
        expect(toastMock.error).not.toHaveBeenCalled();
    });

    it("should display the regeneration rejection even in compact mode", async () => {
        const reason = "Pi 会话启动失败：找不到运行程序";
        mockInvokeHandler("regenerate_ai", () => { throw { ProviderError: reason }; });
        const { result, props } = setup(true);
        await act(async () => { result.current.handleMessageRegenerate(26602); });
        expect(toastMock.error).toHaveBeenCalledWith("重新生成失败", { description: reason });
        expect(props.setAiIsResponsing).toHaveBeenLastCalledWith(false);
        expect(props.clearShiningMessages).toHaveBeenCalled();
    });

    it("keeps a normal response in the responding state", async () => {
        mockInvokeHandler("ask_ai", () => ({ conversation_id: 7, request_prompt_result_with_context: "" }));
        const { result, props } = setup();
        await act(async () => {
            result.current.handleSend();
        });
        await waitFor(() => expect(props.onChangeConversationId).toHaveBeenCalledWith("7"));
        expect(props.setAiIsResponsing).toHaveBeenLastCalledWith(true);
        expect(toastMock.error).not.toHaveBeenCalled();
    });

    it.each(["default", "acceptEdits", "auto", "bypassPermissions", "dontAsk"])("should pass Claude mode %s to the native command", async mode => {
        mockInvokeHandler("ask_ai", () => ({ conversation_id: 7, request_prompt_result_with_context: "" }));
        const { result } = setup(false, mode);
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("ask_ai", expect.objectContaining({
            overrideModelConfig: { permission_mode: mode },
        })));
    });

    it.each([
        { state: "before the native session connects", draft: "dontAsk", expected: { permission_mode: "dontAsk" } },
        { state: "after permissions are bound to the native session", draft: undefined, expected: undefined },
    ])("should queue the message with the open conversation's assistant $state", async ({ draft, expected }) => {
        mockInvokeHandler("enqueue_conversation_message", () => ({ id: 8, conversation_id: 7 }));
        const { result } = setup(false, draft, {
            selectedAssistant: 99,
            conversation: { id: 7, assistant_id: 1 },
            assistants: [{ id: 1, name: "Claude", assistant_type: 4 }],
            aiIsResponsing: true,
        });
        await act(async () => { result.current.handleSend(); });
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("enqueue_conversation_message", expect.objectContaining({
            request: expect.objectContaining({ assistant_id: 1, conversation_id: "7" }),
            overrideModelConfig: expected,
        })));
    });

    it("turns a serialized NoModelFound rejection into a readable message", async () => {
        mockInvokeHandler("ask_ai", () => {
            throw "NoModelFound";
        });
        const { result, props } = setup();
        await act(async () => {
            result.current.handleSend();
        });
        await waitFor(() => expect(toastMock.error).toHaveBeenCalled());
        expect(toastMock.error).toHaveBeenCalledWith("快速使用助手 运行失败", {
            description: NO_MODEL_CONFIGURED_MESSAGE,
        });
        expect(props.setAiIsResponsing).toHaveBeenLastCalledWith(false);
        expect(props.onChangeConversationId).not.toHaveBeenCalled();
    });
});
