import { act, renderHook } from "@testing-library/react";
import { emit, listen } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDataRefresh } from "./useDataRefresh";

const tick = () => act(async () => { await vi.advanceTimersByTimeAsync(110); });

describe("useDataRefresh", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("should coalesce events and reject stale responses when invalidated during a read", async () => {
        let finish: (() => void) | undefined;
        const values: number[] = [];
        let reads = 0;
        const refresh = vi.fn(async (isCurrent: () => boolean) => {
            reads += 1;
            if (reads === 1) await new Promise<void>((resolve) => { finish = resolve; });
            if (isCurrent()) values.push(reads);
        });
        renderHook(() => useDataRefresh({ events: ["changed"], refresh, initial: true }));
        await tick();
        await act(async () => { await emit("changed"); await emit("changed"); });
        await tick();
        expect(refresh).toHaveBeenCalledTimes(1);
        await act(async () => { finish?.(); });
        expect(values).toEqual([]);
        await tick();
        expect(refresh).toHaveBeenCalledTimes(2);
        expect(values).toEqual([2]);
    });

    it("should keep one pending refresh when paused until the response finishes", async () => {
        const refresh = vi.fn();
        const { rerender } = renderHook(({ paused }) => useDataRefresh({ events: ["changed"], refresh, paused }), {
            initialProps: { paused: true },
        });
        await act(async () => { await emit("changed"); await emit("changed"); });
        await tick();
        expect(refresh).not.toHaveBeenCalled();
        rerender({ paused: false });
        await tick();
        expect(refresh).toHaveBeenCalledTimes(1);
        rerender({ paused: true });
        rerender({ paused: false });
        await tick();
        expect(refresh).toHaveBeenCalledTimes(1);
    });

    it("should discard an old conversation read when identity changes", async () => {
        let finish: (() => void) | undefined;
        const committed: string[] = [];
        const { rerender } = renderHook(({ id }) => useDataRefresh({
            events: ["changed"], identity: id, initial: true,
            refresh: async (isCurrent) => {
                if (id === "old") await new Promise<void>((resolve) => { finish = resolve; });
                if (isCurrent()) committed.push(id);
            },
        }), { initialProps: { id: "old" } });
        await tick();
        rerender({ id: "new" });
        await tick();
        await act(async () => { finish?.(); });
        expect(committed).toEqual(["new"]);
    });

    it("should discard a pending read when generation starts and reload after it finishes", async () => {
        let finish: (() => void) | undefined;
        const committed: number[] = [];
        let reads = 0;
        const { rerender } = renderHook(({ paused }) => useDataRefresh({
            events: ["changed"], initial: true, paused,
            refresh: async (isCurrent) => {
                const read = ++reads;
                if (read === 1) await new Promise<void>((resolve) => { finish = resolve; });
                if (isCurrent()) committed.push(read);
            },
        }), { initialProps: { paused: false } });
        rerender({ paused: true });
        await act(async () => { finish?.(); });
        await tick();
        expect(committed).toEqual([]);
        expect(reads).toBe(1);
        rerender({ paused: false });
        await tick();
        expect(committed).toEqual([2]);
    });

    it("should clean up a listener when registration resolves after unmount", async () => {
        let register: ((unlisten: () => void) => void) | undefined;
        const unsubscribe = vi.fn();
        vi.mocked(listen).mockImplementationOnce(() => new Promise((resolve) => { register = resolve; }));
        const { unmount } = renderHook(() => useDataRefresh({ events: ["late"], refresh: vi.fn() }));
        unmount();
        await act(async () => { register?.(unsubscribe); });
        expect(unsubscribe).toHaveBeenCalled();
    });

    it("should report the original error and keep the listener ready for a later change", async () => {
        const onError = vi.fn();
        const refresh = vi.fn().mockRejectedValueOnce(new Error("database unavailable")).mockResolvedValue(undefined);
        renderHook(() => useDataRefresh({ events: ["changed"], initial: true, refresh, onError }));
        await tick();
        expect(onError).toHaveBeenCalledWith(expect.objectContaining({ message: "database unavailable" }));
        await act(async () => { await emit("changed"); });
        await tick();
        expect(refresh).toHaveBeenCalledTimes(2);
    });
});
