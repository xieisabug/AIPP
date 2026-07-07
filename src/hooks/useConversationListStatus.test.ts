import { act, renderHook } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ConversationListActivityEvent } from "@/data/Conversation";
import { useConversationListStatus } from "@/hooks/useConversationListStatus";
import {
    clearAllMockHandlers,
    mockInvokeHandler,
} from "@/__tests__/mocks/tauri";

const flushEffects = async () => {
    await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
    });
};

async function emitListActivity(payload: ConversationListActivityEvent) {
    await act(async () => {
        await emit("conversation_list_activity", payload);
        await Promise.resolve();
    });
}

describe("useConversationListStatus", () => {
    beforeEach(() => {
        mockInvokeHandler("list_running_conversation_ids", () => []);
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("returns idle by default", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        expect(result.current.getItemStatus(42)).toBe("idle");
    });

    it("initializes responding state from list_running_conversation_ids", async () => {
        mockInvokeHandler("list_running_conversation_ids", () => [7, 9]);

        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        expect(result.current.getItemStatus(7)).toBe("responding");
        expect(result.current.getItemStatus(9)).toBe("responding");
        expect(result.current.getItemStatus(1)).toBe("idle");
    });

    it("marks responding when runtime_state reports is_running=true", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: true,
        });

        expect(result.current.getItemStatus(5)).toBe("responding");
    });

    it("clears responding when runtime_state reports is_running=false without marking unread", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: true,
        });
        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: false,
        });

        expect(result.current.getItemStatus(5)).toBe("idle");
    });

    it("marks completed_unread on stream_complete for non-active conversation", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "stream_complete",
        });

        expect(result.current.getItemStatus(5)).toBe("completed_unread");
    });

    it("does not mark completed_unread when stream_complete is for active conversation", async () => {
        const { result } = renderHook(() => useConversationListStatus("5"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "stream_complete",
        });

        expect(result.current.getItemStatus(5)).toBe("idle");
    });

    it("clears completed_unread when conversation becomes active", async () => {
        const { result, rerender } = renderHook(
            ({ activeId }) => useConversationListStatus(activeId),
            { initialProps: { activeId: "1" } },
        );
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "stream_complete",
        });
        expect(result.current.getItemStatus(5)).toBe("completed_unread");

        rerender({ activeId: "5" });
        await flushEffects();

        expect(result.current.getItemStatus(5)).toBe("idle");
    });

    it("prefers responding over completed_unread", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "stream_complete",
        });
        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: true,
        });

        expect(result.current.getItemStatus(5)).toBe("responding");
    });

    it("clears unread when conversation starts responding again", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "stream_complete",
        });
        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: true,
        });
        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: false,
        });

        expect(result.current.getItemStatus(5)).toBe("idle");
    });

    it("removes state when conversation is deleted", async () => {
        const { result } = renderHook(() => useConversationListStatus("1"));
        await flushEffects();

        await emitListActivity({
            conversation_id: 5,
            kind: "runtime_state",
            is_running: true,
        });
        await emitListActivity({
            conversation_id: 5,
            kind: "stream_complete",
        });

        await act(async () => {
            await emit("conversation_deleted", 5);
            await Promise.resolve();
        });

        expect(result.current.getItemStatus(5)).toBe("idle");
    });
});