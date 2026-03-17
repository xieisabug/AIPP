import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useScrollManagement } from "./useScrollManagement";

describe("useScrollManagement", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        vi.stubGlobal(
            "requestAnimationFrame",
            vi.fn((callback: FrameRequestCallback) => {
                callback(0);
                return 1;
            }),
        );
    });

    afterEach(() => {
        vi.runOnlyPendingTimers();
        vi.useRealTimers();
        vi.unstubAllGlobals();
    });

    it("replays smartScroll after scrollToUserMessage releases its smooth-scroll lock", () => {
        const container = {
            scrollTop: 120,
            scrollHeight: 1000,
            clientHeight: 400,
            scrollTo: vi.fn((options: ScrollToOptions) => {
                if (typeof options.top === "number") {
                    container.scrollTop = options.top;
                }
            }),
            querySelector: vi.fn(() => null),
            querySelectorAll: vi.fn(() => []),
            lastElementChild: null,
        } as unknown as HTMLDivElement;

        const { result } = renderHook(() => useScrollManagement());

        act(() => {
            result.current.scrollContainerRef.current = container;
        });

        act(() => {
            result.current.scrollToUserMessage();
        });

        expect(container.scrollTo).toHaveBeenNthCalledWith(1, {
            top: 1000,
            behavior: "smooth",
        });

        act(() => {
            result.current.smartScroll();
        });

        expect(container.scrollTo).toHaveBeenCalledTimes(1);

        act(() => {
            vi.advanceTimersByTime(349);
        });

        expect(container.scrollTo).toHaveBeenCalledTimes(1);

        act(() => {
            vi.advanceTimersByTime(1);
        });

        expect(container.scrollTo).toHaveBeenCalledTimes(2);
        expect(container.scrollTo).toHaveBeenNthCalledWith(2, {
            top: 1000,
            behavior: "auto",
        });
    });

    it("cancels queued auto-scroll when the user starts scrolling manually", () => {
        const container = {
            scrollTop: 120,
            scrollHeight: 1000,
            clientHeight: 400,
            scrollTo: vi.fn((options: ScrollToOptions) => {
                if (typeof options.top === "number") {
                    container.scrollTop = options.top;
                }
            }),
            querySelector: vi.fn(() => null),
            querySelectorAll: vi.fn(() => []),
            lastElementChild: null,
        } as unknown as HTMLDivElement;

        const { result } = renderHook(() => useScrollManagement());

        act(() => {
            result.current.scrollContainerRef.current = container;
        });

        act(() => {
            result.current.scrollToUserMessage();
            result.current.smartScroll();
            result.current.handleUserScrollIntent();
        });

        act(() => {
            vi.advanceTimersByTime(400);
        });

        expect(container.scrollTo).toHaveBeenCalledTimes(1);
        expect(container.scrollTo).toHaveBeenNthCalledWith(1, {
            top: 1000,
            behavior: "smooth",
        });
    });

    it("suppresses immediate forced auto-scroll right after user scroll intent", () => {
        const container = {
            scrollTop: 120,
            scrollHeight: 1000,
            clientHeight: 400,
            scrollTo: vi.fn((options: ScrollToOptions) => {
                if (typeof options.top === "number") {
                    container.scrollTop = options.top;
                }
            }),
            querySelector: vi.fn(() => null),
            querySelectorAll: vi.fn(() => []),
            lastElementChild: null,
        } as unknown as HTMLDivElement;

        const { result } = renderHook(() => useScrollManagement());

        act(() => {
            result.current.scrollContainerRef.current = container;
            result.current.handleUserScrollIntent();
            result.current.smartScroll(true, "auto");
        });

        expect(container.scrollTo).not.toHaveBeenCalled();

        act(() => {
            vi.advanceTimersByTime(251);
            result.current.smartScroll(true, "auto");
        });

        expect(container.scrollTo).toHaveBeenCalledTimes(1);
        expect(container.scrollTo).toHaveBeenNthCalledWith(1, {
            top: 1000,
            behavior: "auto",
        });
    });
});
