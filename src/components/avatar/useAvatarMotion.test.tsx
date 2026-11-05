import { act, renderHook } from "@testing-library/react";
import { useRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAvatarMotion, type AvatarActionRequest } from "./useAvatarMotion";
import type { AvatarState } from "./avatarDefinition";

const preferences = vi.hoisted(() => ({ reduced: false }));
vi.mock("motion/react", () => ({ useReducedMotion: () => preferences.reduced }));

describe("avatar action lifecycle", () => {
    beforeEach(() => { vi.useFakeTimers(); preferences.reduced = false; });
    afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });
    const setup = () => renderHook(({ state, enabled, request }: { state: AvatarState; enabled: boolean; request?: AvatarActionRequest }) => {
        const ref = useRef<HTMLElement>(null);
        return useAvatarMotion(ref, enabled, state, request);
    }, { initialProps: { state: "idle" as AvatarState, enabled: true, request: undefined as AvatarActionRequest | undefined } });

    it("should settle equipped props when another state interrupts a performance", () => {
        const { result, rerender, unmount } = setup();
        const request: AvatarActionRequest = { action: "glasses", sequence: 1 };
        rerender({ state: "idle", enabled: true, request });
        expect(result.current.action).toBe("glasses");
        act(() => vi.advanceTimersByTime(400));
        rerender({ state: "thinking", enabled: true, request });
        expect(result.current.action).toBeNull();
        unmount();
        expect(vi.getTimerCount()).toBe(0);
    });

    it("should not replay an old performance when animations are enabled again", () => {
        const { result, rerender } = setup();
        const request: AvatarActionRequest = { action: "hat", sequence: 1 };
        rerender({ state: "idle", enabled: true, request });
        expect(result.current.action).toBe("hat");
        rerender({ state: "idle", enabled: false, request });
        expect(result.current.action).toBeNull();
        rerender({ state: "idle", enabled: true, request });
        expect(result.current.action).toBeNull();
    });

    it("should suppress motion when reduced motion is requested", () => {
        preferences.reduced = true;
        const { result, rerender } = setup();
        rerender({ state: "success", enabled: true, request: { action: "glasses", sequence: 1 } });
        expect(result.current.active).toBe(false);
        expect(result.current.action).toBeNull();
        expect(vi.getTimerCount()).toBe(0);
    });

    it("should stop and discard an action when the page becomes hidden", () => {
        const { result, rerender } = setup();
        rerender({ state: "idle", enabled: true, request: { action: "glasses", sequence: 1 } });
        expect(result.current.action).toBe("glasses");
        const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(true);
        act(() => document.dispatchEvent(new Event("visibilitychange")));
        expect(result.current.active).toBe(false);
        expect(result.current.action).toBeNull();
        expect(vi.getTimerCount()).toBe(0);
        hidden.mockReturnValue(false);
        act(() => document.dispatchEvent(new Event("visibilitychange")));
        expect(result.current.active).toBe(true);
        expect(result.current.action).toBeNull();
    });

    it("should pause offscreen instances and disconnect observers when unmounted", () => {
        let notify: (entries: { isIntersecting: boolean }[]) => void = () => {};
        const disconnect = vi.fn();
        vi.stubGlobal("IntersectionObserver", class {
            constructor(callback: typeof notify) { notify = callback; }
            observe() {}
            disconnect = disconnect;
        });
        const { result, unmount } = setup();
        act(() => notify([{ isIntersecting: false }]));
        expect(result.current.active).toBe(false);
        expect(vi.getTimerCount()).toBe(0);
        unmount();
        expect(disconnect).toHaveBeenCalledOnce();
    });
});
