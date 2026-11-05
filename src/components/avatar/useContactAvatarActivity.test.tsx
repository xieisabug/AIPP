import { act, renderHook, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useContactAvatarActivity } from "./useContactAvatarActivity";
import type { ContactActivity } from "./contactActivity";

const row = (status: string | null = null): ContactActivity => ({ assistant_id: 8, conversation_id: 3, status,
    permission: false, question: false, waiting_peer: false,
    runtime: { conversation_id: 3, is_running: false, phase: "idle", epoch: 1, revision: 1 } });
describe("contact activity subscriptions", () => {
    afterEach(() => { clearAllMockHandlers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
    it("should baseline historical results and animate new task completion only once", async () => {
        let now = 1000;
        vi.spyOn(Date, "now").mockImplementation(() => now);
        let rows = [row("failed")];
        mockInvokeHandler("list_butler_contact_activity", () => rows);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("idle"));
        rows = [row("running")];
        await act(async () => { await emit("butler_task_updated", {}); });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("working"));
        rows = [row("succeeded")];
        await act(async () => { await emit("butler_task_updated", {}); });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("finished"));
        now += 6000;
        await act(async () => { await emit("butler_task_updated", {}); });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("idle"));
    });
    it("should not celebrate cancelled or failed private conversations", async () => {
        mockInvokeHandler("list_butler_contact_activity", () => [row()]);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.has(8)).toBe(true));
        await act(async () => {
            await emit("conversation_event_3", { type: "conversation_cancel", data: {} });
            await emit("conversation_event_3", { type: "stream_complete", data: { has_response: true } });
        });
        expect(result.current.visuals.get(8)?.state).toBe("idle");
        await act(async () => {
            await emit("conversation_event_3", { type: "runtime_state_snapshot", data: { state: { ...row().runtime, epoch: 2, is_running: true, phase: "user_pending" } } });
            await emit("conversation_event_3", { type: "message_add", data: { message_type: "error" } });
            await emit("conversation_event_3", { type: "stream_complete", data: { has_response: true } });
        });
        expect(result.current.visuals.get(8)?.state).toBe("failed");
        act(() => result.current.acknowledge?.(8));
        expect(result.current.visuals.get(8)?.label).not.toBe("执行出错");
    });
    it("should notify only a new unread message and preserve its baseline on selection", async () => {
        vi.stubGlobal("localStorage", { getItem: vi.fn(() => null) });
        mockInvokeHandler("list_butler_contact_activity", () => [row()]);
        const preview = (messageId: number) => [{ assistantId: 8, conversationId: 3, messageId, preview: "消息" }];
        const { result, rerender } = renderHook(({ messageId }) => useContactAvatarActivity("1", preview(messageId)), { initialProps: { messageId: 10 } });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("idle"));
        rerender({ messageId: 11 });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("notifying"));
    });
});
