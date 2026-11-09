import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useAcpElicitation } from "./useAcpElicitation";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), listeners: new Map<string, (event: { payload: unknown }) => void>() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name, callback) => {
    mocks.listeners.set(name, callback);
    return () => mocks.listeners.delete(name);
}) }));
const form = (id: string, conversation = 200) => ({ request_id: id, conversation_id: conversation, agent_kind: "acp", message: "选择方案", schema: { type: "object", properties: { choice: { type: "string", title: "方案" } }, required: ["choice"] } });
beforeEach(() => { mocks.invoke.mockReset(); mocks.listeners.clear(); });

it("should recover a pending form and submit its exact identity when reopening an execution", async () => {
    mocks.invoke.mockResolvedValueOnce([form("task-question")]).mockResolvedValueOnce(undefined);
    const { result } = renderHook(() => useAcpElicitation({ conversationId: 200 }));
    await waitFor(() => expect(result.current.questionRequest?.request_id).toBe("task-question"));
    expect(mocks.invoke).toHaveBeenCalledWith("list_acp_elicitation_requests", { conversationId: 200 });
    await act(() => result.current.handleSubmit("task-question", { choice: "方案 A" }));
    expect(mocks.invoke).toHaveBeenCalledWith("confirm_acp_elicitation", { requestId: "task-question", action: "accept", values: { choice: "方案 A" } });
    expect(result.current.questionRequest).toBeNull();
});

it("should ignore resolved forms in a late snapshot without losing new live requests", async () => {
    let finish!: (value: ReturnType<typeof form>[]) => void;
    mocks.invoke.mockImplementation(() => new Promise(resolve => { finish = resolve; }));
    const { result } = renderHook(() => useAcpElicitation({ conversationId: 200 }));
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalled());
    await act(async () => {
        mocks.listeners.get("acp-elicitation-resolved")!({ payload: { request_id: "old", conversation_id: 200 } });
        mocks.listeners.get("acp-elicitation-request")!({ payload: form("live") });
        finish([form("old"), form("live"), form("other", 300)]);
    });
    expect(result.current.questionRequest?.request_id).toBe("live");
    mocks.invoke.mockResolvedValueOnce(undefined);
    await act(() => result.current.handleCancel("live"));
    expect(result.current.questionRequest).toBeNull();
});

it("should discard the old execution snapshot when switching conversations", async () => {
    let finish!: (value: ReturnType<typeof form>[]) => void;
    mocks.invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; })).mockResolvedValueOnce([form("new", 300)]);
    const { result, rerender } = renderHook(({ id }) => useAcpElicitation({ conversationId: id }), { initialProps: { id: 200 } });
    await waitFor(() => expect(mocks.invoke).toHaveBeenCalledTimes(1));
    rerender({ id: 300 });
    await waitFor(() => expect(result.current.questionRequest?.request_id).toBe("new"));
    await act(async () => finish([form("old")]));
    expect(result.current.questionRequest?.request_id).toBe("new");
});

it("should keep a new execution free of errors from an earlier pending submission", async () => {
    let reject!: (error: Error) => void;
    mocks.invoke.mockResolvedValueOnce([form("old")])
        .mockImplementationOnce(() => new Promise((_resolve, rejectPromise) => { reject = rejectPromise; }))
        .mockResolvedValueOnce([form("new", 300)]);
    const { result, rerender } = renderHook(({ id }) => useAcpElicitation({ conversationId: id }), { initialProps: { id: 200 } });
    await waitFor(() => expect(result.current.questionRequest?.request_id).toBe("old"));
    let submission!: Promise<void>;
    act(() => { submission = result.current.handleCancel("old"); });
    rerender({ id: 300 });
    await waitFor(() => expect(result.current.questionRequest?.request_id).toBe("new"));
    await act(async () => { reject(new Error("old execution failed")); await submission; });
    expect(result.current.decisionError).toBeNull();
    expect(result.current.questionRequest?.request_id).toBe("new");
});
