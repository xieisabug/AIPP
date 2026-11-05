import { act, renderHook, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it } from "vitest";
import type { ConversationRuntimeState } from "@/data/Conversation";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useConversationAvatarState } from "./useConversationAvatarState";

describe("conversation-driven avatars", () => {
    afterEach(clearAllMockHandlers);
    it("should follow runtime revisions and avoid celebrating cancellation", async () => {
        mockInvokeHandler("get_conversation_runtime_state", () => ({ conversation_id: 12, is_running: true, phase: "user_pending", epoch: 1, revision: 1 }));
        const { result, rerender } = renderHook(({ id, waiting }) => useConversationAvatarState(id, waiting), { initialProps: { id: "12", waiting: false } });
        await waitFor(() => expect(result.current).toBe("thinking"));
        const snapshot = async (revision: number, phase: string) => act(async () => {
            await emit("conversation_event_12", { type: "runtime_state_snapshot", data: { state: { conversation_id: 12, is_running: true, phase, epoch: 1, revision } } });
        });
        await snapshot(3, "assistant_streaming");
        expect(result.current).toBe("responding");
        await snapshot(2, "user_pending");
        expect(result.current).toBe("responding");
        rerender({ id: "12", waiting: true });
        expect(result.current).toBe("waiting");
        rerender({ id: "12", waiting: false });
        await act(async () => {
            await emit("conversation_event_12", { type: "conversation_cancel", data: {} });
            await emit("conversation_event_12", { type: "stream_complete", data: { has_response: true } });
        });
        expect(result.current).toBe("idle");
        rerender({ id: "13", waiting: false });
        expect(result.current).toBe("idle");
    });

    it("should retain an error expression when the failed stream completes", async () => {
        mockInvokeHandler("get_conversation_runtime_state", () => ({ conversation_id: 5, is_running: true, phase: "assistant_streaming", epoch: 1, revision: 1 }));
        const { result } = renderHook(() => useConversationAvatarState("5", false));
        await waitFor(() => expect(result.current).toBe("responding"));
        await act(async () => {
            await emit("conversation_event_5", { type: "message_update", data: { message_type: "error" } });
            await emit("conversation_event_5", { type: "stream_complete", data: { has_response: true } });
        });
        expect(result.current).toBe("error");
    });

    it("should ignore a stale initial snapshot when a live state has already arrived", async () => {
        let resolveSnapshot: (state: ConversationRuntimeState) => void = () => {};
        let requested = false;
        mockInvokeHandler("get_conversation_runtime_state", () => {
            requested = true;
            return new Promise<ConversationRuntimeState>(resolve => { resolveSnapshot = resolve; });
        });
        const { result } = renderHook(() => useConversationAvatarState("5", false));
        await waitFor(() => expect(requested).toBe(true));
        await act(async () => {
            await emit("conversation_event_5", { type: "runtime_state_snapshot", data: { state: { conversation_id: 5, is_running: true, phase: "assistant_streaming", epoch: 2, revision: 3 } } });
            resolveSnapshot({ conversation_id: 5, is_running: false, phase: "idle", epoch: 1, revision: 1 });
        });
        expect(result.current).toBe("responding");
    });
});
