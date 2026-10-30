import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useConversationOperations } from "./useConversationOperations";

const platformState = vi.hoisted(() => ({ mobile: true }));
vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => platformState.mobile }));
const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn(), warning: vi.fn(), info: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));
vi.mock("./useConversationManager", () => ({
    default: () => ({ deleteConversation: vi.fn(), updateConversationName: vi.fn() }),
}));

function setup(assistantType: number) {
    const setAiIsResponsing = vi.fn();
    const props: any = {
        conversation: null,
        selectedAssistant: 2,
        assistants: [{ id: 2, name: "Codex CLI", assistant_type: assistantType }],
        setMessages: vi.fn(),
        inputText: "hello",
        setInputText: vi.fn(),
        fileInfoList: [],
        clearFileInfoList: vi.fn(),
        aiIsResponsing: false,
        setAiIsResponsing,
        onChangeConversationId: vi.fn(),
        setShiningMessageIds: vi.fn(),
        setManualShineMessage: vi.fn(),
        updateShiningMessages: vi.fn(),
        clearShiningMessages: vi.fn(),
        assistantTypePluginMap: new Map(),
        assistantRunApi: {},
    };
    const { result } = renderHook(() => useConversationOperations(props));
    return { result, setAiIsResponsing };
}

describe("useConversationOperations mobile capability gate", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        toastMock.error.mockClear();
        mockInvokeHandler("ask_ai", () => ({ conversation_id: 1, add_message_id: 1, request_prompt_result_with_context: "" }));
    });

    it("blocks a local CLI agent before ask_ai with MOBILE_UNSUPPORTED and no fallback", async () => {
        platformState.mobile = true;
        const { result, setAiIsResponsing } = setup(4);
        await act(async () => {
            result.current.handleSend();
        });
        expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("MOBILE_UNSUPPORTED: 本地 CLI Agent"));
        expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("ask_ai", expect.anything());
        expect(setAiIsResponsing).toHaveBeenLastCalledWith(false);
    });

    it("does not block the same assistant on desktop", async () => {
        platformState.mobile = false;
        const { result } = setup(4);
        await act(async () => {
            result.current.handleSend();
        });
        expect(toastMock.error).not.toHaveBeenCalledWith(expect.stringContaining("MOBILE_UNSUPPORTED"));
    });
});
