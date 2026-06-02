import { act, render, renderHook, screen } from "@testing-library/react";
import { createElement, useLayoutEffect } from "react";
import type { WheelEvent } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useScrollManagement } from "./useScrollManagement";
import {
    CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR,
    CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR,
} from "@/components/conversation/layoutConstants";

class ResizeObserverMock {
    observe = vi.fn();
    disconnect = vi.fn();
}

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
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
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

    it("keeps a padded live-only viewport height separate from the normal viewport height", () => {
        function Harness() {
            const { scrollContainerRef } = useScrollManagement();

            useLayoutEffect(() => {
                const container = scrollContainerRef.current;
                if (!container) return;
                Object.defineProperty(container, "clientHeight", {
                    value: 500,
                    configurable: true,
                });
            }, [scrollContainerRef]);

            return createElement("div", {
                ref: scrollContainerRef,
                "data-testid": "scroll-container",
                style: {
                    paddingTop: 24,
                    paddingBottom: 24,
                    rowGap: 16,
                },
            });
        }

        render(createElement(Harness));

        const container = screen.getByTestId("scroll-container");

        expect(
            container.style.getPropertyValue(CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR),
        ).toBe("490px");
        expect(
            container.style.getPropertyValue(
                CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR,
            ),
        ).toBe("426px");
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

    it("keeps normal smartScroll disabled after the user wheels upward from bottom", () => {
        const container = {
            scrollTop: 600,
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
            result.current.handleUserScrollIntent({
                deltaY: -80,
            } as WheelEvent<HTMLDivElement>);
        });

        act(() => {
            vi.advanceTimersByTime(251);
            result.current.smartScroll();
        });

        expect(container.scrollTo).not.toHaveBeenCalled();
    });
});
