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
    it("should hold a running state across the brief idle gap between tool rounds", async () => {
        let now = 1000;
        vi.spyOn(Date, "now").mockImplementation(() => now);
        let rows = [row("running")];
        mockInvokeHandler("list_butler_contact_activity", () => rows);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("working"));
        rows = [row()];
        now += 100;
        await act(async () => { await emit("butler_task_updated", {}); });
        expect(result.current.visuals.get(8)?.state).toBe("working");
        now += 1300;
        await act(async () => { await emit("butler_task_updated", {}); });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("idle"));
    });
    it("should show the current run's progress summary but never over an approval", async () => {
        const running = { ...row(), runtime: { ...row().runtime, is_running: true, phase: "mcp_executing" as const, epoch: 2 } };
        let rows = [running];
        mockInvokeHandler("list_butler_contact_activity", () => rows);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.get(8)?.label).toBe("执行中"));
        await act(async () => { await emit("butler_progress_summary", { conversation_id: 3, epoch: 1, text: "上一轮", steps: [] }); });
        expect(result.current.visuals.get(8)?.label).toBe("执行中");
        await act(async () => { await emit("butler_progress_summary", { conversation_id: 3, epoch: 2, text: "正在对比报价", steps: [] }); });
        expect(result.current.visuals.get(8)?.label).toBe("正在对比报价");
        rows = [{ ...running, permission: true }];
        await act(async () => { await emit("butler_task_updated", {}); });
        await waitFor(() => expect(result.current.visuals.get(8)?.label).toBe("待审批"));
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
    it("should follow the executing tool and hold each action for a minimum dwell", async () => {
        let now = 1000;
        vi.spyOn(Date, "now").mockImplementation(() => now);
        const executing = { ...row(), runtime: { ...row().runtime, is_running: true, phase: "mcp_executing" as const, epoch: 2 } };
        mockInvokeHandler("list_butler_contact_activity", () => [executing]);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("working"));
        now += 2000;
        await act(async () => { await emit("conversation_event_3", { type: "mcp_tool_call_update", data: { call_id: 1, status: "executing", tool_name: "read_file", parameters: "{\"file_path\":\"a.md\"}" } }); });
        expect(result.current.visuals.get(8)?.state).toBe("reading");
        now += 100;
        await act(async () => {
            await emit("conversation_event_3", { type: "mcp_tool_call_update", data: { call_id: 1, status: "success" } });
            await emit("conversation_event_3", { type: "mcp_tool_call_update", data: { call_id: 2, status: "executing", tool_name: "search_web" } });
        });
        expect(result.current.visuals.get(8)?.state).toBe("reading");
        now += 1200;
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("searching"), { timeout: 2500 });
        await act(async () => { await emit("conversation_event_3", { type: "tool_review_update", data: { call_id: 2, phase: "reviewing" } }); });
        now += 1300;
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("suspicious"), { timeout: 2500 });
    });
    it("should celebrate a finished run whose todo list was fully checked off", async () => {
        mockInvokeHandler("list_butler_contact_activity", () => [row()]);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.has(8)).toBe(true));
        const todos = JSON.stringify({ todos: [{ content: "a", status: "completed" }, { content: "b", status: "completed" }] });
        await act(async () => {
            await emit("conversation_event_3", { type: "mcp_tool_call_update", data: { call_id: 5, status: "success", tool_name: "todo_write", parameters: todos } });
            await emit("conversation_event_3", { type: "stream_complete", data: { has_response: true } });
        });
        expect(result.current.visuals.get(8)?.state).toBe("celebrate");
    });
    it("should show a one-shot interruption when a task is interrupted", async () => {
        let rows = [row("running")];
        mockInvokeHandler("list_butler_contact_activity", () => rows);
        const { result } = renderHook(() => useContactAvatarActivity("1", []));
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("working"));
        rows = [row("interrupted")];
        await act(async () => { await emit("butler_task_updated", {}); });
        await waitFor(() => expect(result.current.visuals.get(8)?.state).toBe("blocked"));
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
