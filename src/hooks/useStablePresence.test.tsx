import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useLinger, useSettledFlag } from "./useStablePresence";

describe("useLinger", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("bridges a short gap to an empty value", () => {
        const { result, rerender } = renderHook(({ value }) => useLinger(value, 1000, "c1"), { initialProps: { value: "执行中" as string | undefined } });
        rerender({ value: undefined });
        expect(result.current).toBe("执行中");
        rerender({ value: "思考中" });
        expect(result.current).toBe("思考中");
    });

    it("settles to the empty value after the gap", () => {
        const { result, rerender } = renderHook(({ value }) => useLinger(value, 1000, "c1"), { initialProps: { value: true } });
        rerender({ value: false });
        act(() => { vi.advanceTimersByTime(1000); });
        expect(result.current).toBe(false);
    });

    it("drops the held value when the scope changes", () => {
        const { result, rerender } = renderHook(({ value, scope }) => useLinger(value, 1000, scope), { initialProps: { value: true, scope: "c1" } });
        rerender({ value: false, scope: "c2" });
        expect(result.current).toBe(false);
    });
});

describe("useSettledFlag", () => {
    beforeEach(() => vi.useFakeTimers());
    afterEach(() => vi.useRealTimers());

    it("ignores a flag that flips back quickly", () => {
        const { result, rerender } = renderHook(({ flag }) => useSettledFlag(flag, 1500), { initialProps: { flag: true } });
        act(() => { vi.advanceTimersByTime(200); });
        rerender({ flag: false });
        act(() => { vi.advanceTimersByTime(2000); });
        expect(result.current).toBe(false);
    });

    it("turns on after the flag persists", () => {
        const { result } = renderHook(() => useSettledFlag(true, 1500));
        expect(result.current).toBe(false);
        act(() => { vi.advanceTimersByTime(1500); });
        expect(result.current).toBe(true);
    });
});
