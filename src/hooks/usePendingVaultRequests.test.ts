import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { usePendingVaultRequests } from "./usePendingVaultRequests";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const channels = { list: "vault_list_use_requests", request: "vault-use-request", resolved: "vault-use-resolved" };
type Request = { requestId: string; conversationId: number | null; name?: string };
type Handler = (event: { payload: Request }) => void;
const handlers = new Map<string, Handler>();
const off = vi.fn();

function deferred<T>() {
    let resolve!: (value: T) => void;
    let reject!: (reason: unknown) => void;
    const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no; });
    return { promise, resolve, reject };
}

const request = (requestId: string, conversationId = 5): Request => ({ requestId, conversationId });
function emit(channel: string, payload: Request) {
    act(() => handlers.get(channel)?.({ payload }));
}

describe("usePendingVaultRequests", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        handlers.clear();
        vi.mocked(listen).mockImplementation(async (event, handler) => {
            handlers.set(event, handler as Handler);
            return off;
        });
    });

    it("should preserve new requests and suppress resolved rows when a stale snapshot arrives", async () => {
        const snapshot = deferred<Request[]>();
        vi.mocked(invoke).mockReturnValue(snapshot.promise);
        const { result } = renderHook(() => usePendingVaultRequests<Request>(5, channels));
        await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
        emit(channels.request, request("new"));
        emit(channels.request, request("closed"));
        emit(channels.resolved, request("closed"));
        // A resolution can arrive for a row that was only present in the backend snapshot.
        emit(channels.resolved, request("snapshot-only"));
        await act(async () => snapshot.resolve([request("old"), request("closed"), request("snapshot-only"), request("other", 9)]));
        expect(result.current[0].map((item) => item.requestId)).toEqual(["old", "new"]);
    });

    it("should deduplicate requests and retain event data when the snapshot contains the same id", async () => {
        const snapshot = deferred<Request[]>();
        vi.mocked(invoke).mockReturnValue(snapshot.promise);
        const { result } = renderHook(() => usePendingVaultRequests<Request>(5, channels));
        await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
        emit(channels.request, { ...request("same"), name: "event" });
        emit(channels.request, { ...request("same"), name: "event" });
        await act(async () => snapshot.resolve([{ ...request("same"), name: "snapshot" }]));
        expect(result.current[0]).toEqual([{ ...request("same"), name: "event" }]);
    });

    it("should establish both listeners before loading when subscriptions are delayed", async () => {
        const resolvedReady = deferred<() => void>();
        const requestReady = deferred<() => void>();
        vi.mocked(listen).mockImplementation((event, handler) => {
            handlers.set(event, handler as Handler);
            return event === channels.resolved ? resolvedReady.promise : requestReady.promise;
        });
        vi.mocked(invoke).mockResolvedValue([request("closed"), request("pending")]);
        const { result } = renderHook(() => usePendingVaultRequests<Request>(5, channels));
        expect(listen).toHaveBeenCalledTimes(1);
        expect(invoke).not.toHaveBeenCalled();
        await act(async () => resolvedReady.resolve(off));
        expect(listen).toHaveBeenCalledTimes(2);
        expect(invoke).not.toHaveBeenCalled();
        emit(channels.resolved, request("closed"));
        await act(async () => requestReady.resolve(off));
        await waitFor(() => expect(result.current[0]).toEqual([request("pending")]));
    });

    it("should discard the previous conversation snapshot and callbacks when the conversation changes", async () => {
        const oldSnapshot = deferred<Request[]>();
        const newSnapshot = deferred<Request[]>();
        vi.mocked(invoke).mockReturnValueOnce(oldSnapshot.promise).mockReturnValueOnce(newSnapshot.promise);
        const { result, rerender } = renderHook(({ id }) => usePendingVaultRequests<Request>(id, channels), { initialProps: { id: 5 } });
        await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
        emit(channels.request, request("old"));
        const oldHandler = handlers.get(channels.request)!;
        rerender({ id: 9 });
        expect(result.current[0]).toEqual([]);
        expect(off).toHaveBeenCalledTimes(2);
        await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
        act(() => oldHandler({ payload: request("late-old") }));
        await act(async () => oldSnapshot.resolve([request("stale")]));
        await act(async () => newSnapshot.resolve([request("current", 9)]));
        expect(result.current[0]).toEqual([request("current", 9)]);
    });

    it("should unsubscribe late registrations without loading when unmounted during setup", async () => {
        const ready = deferred<() => void>();
        vi.mocked(listen).mockReturnValue(ready.promise);
        const { unmount } = renderHook(() => usePendingVaultRequests<Request>(5, channels));
        unmount();
        await act(async () => ready.resolve(off));
        expect(off).toHaveBeenCalledOnce();
        expect(invoke).not.toHaveBeenCalled();
    });

    it("should retain matching events when the initial load fails", async () => {
        const snapshot = deferred<Request[]>();
        const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
        vi.mocked(invoke).mockReturnValue(snapshot.promise);
        const { result } = renderHook(() => usePendingVaultRequests<Request>([5, 9], channels));
        await waitFor(() => expect(invoke).toHaveBeenCalledOnce());
        emit(channels.request, request("one"));
        emit(channels.request, request("two", 9));
        emit(channels.request, request("other", 10));
        await act(async () => snapshot.reject(new Error("offline")));
        expect(result.current[0]).toEqual([request("one"), request("two", 9)]);
        emit(channels.resolved, request("one"));
        expect(result.current[0]).toEqual([request("two", 9)]);
        warn.mockRestore();
    });

    it("should clear requests and avoid subscriptions when no conversation is selected", async () => {
        vi.mocked(invoke).mockResolvedValue([request("one")]);
        const { result, rerender } = renderHook(({ id }: { id: number | null }) => usePendingVaultRequests<Request>(id, channels), { initialProps: { id: 5 as number | null } });
        await waitFor(() => expect(result.current[0]).toEqual([request("one")]));
        rerender({ id: null });
        expect(result.current[0]).toEqual([]);
        expect(listen).toHaveBeenCalledTimes(2);
        expect(off).toHaveBeenCalledTimes(2);
    });
});
