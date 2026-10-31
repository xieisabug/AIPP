import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useMobileResume } from "./useMobileResume";

vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => true }));
describe("mobile resume refresh", () => {
    afterEach(() => vi.useRealTimers());
    it("should coalesce resume signals and stop refreshing after unmount", () => {
        vi.useFakeTimers();
        const refresh = vi.fn();
        const { unmount } = renderHook(() => useMobileResume(refresh, "1"));
        act(() => {
            window.dispatchEvent(new Event("focus"));
            window.dispatchEvent(new Event("online"));
            window.dispatchEvent(new Event("aipp:mobile-navigate"));
            vi.advanceTimersByTime(101);
        });
        expect(refresh).toHaveBeenCalledTimes(1);
        unmount();
        window.dispatchEvent(new Event("online"));
        vi.advanceTimersByTime(101);
        expect(refresh).toHaveBeenCalledTimes(1);
    });
});
