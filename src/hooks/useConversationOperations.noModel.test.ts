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

function setup() {
    const props: any = {
        conversation: null,
        selectedAssistant: 1,
        assistants: [{ id: 1, name: "快速使用助手", assistant_type: 0 }],
        setMessages: vi.fn(),
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
            }),
        );
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
