import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import MobileTurnScrubber from "./MobileTurnScrubber";

class ResizeObserverMock {
    observe = vi.fn();
    disconnect = vi.fn();
}

const TURNS = [
    { id: 11, preview: "第一个问题" },
    { id: 22, preview: "第二个问题" },
    { id: 33, preview: "第三个问题" },
];

function makeRef(height: number) {
    const el = document.createElement("div");
    Object.defineProperty(el, "clientHeight", { get: () => height, configurable: true });
    document.body.appendChild(el);
    return { current: el } as React.RefObject<HTMLDivElement | null>;
}

describe("MobileTurnScrubber", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    });

    afterEach(() => {
        vi.useRealTimers();
        vi.unstubAllGlobals();
        document.body.innerHTML = "";
    });

    it("should render nothing when there is only one turn", () => {
        const { container } = render(
            <MobileTurnScrubber turns={TURNS.slice(0, 1)} scrollContainerRef={makeRef(800)} onSelect={vi.fn()} />,
        );
        expect(container.firstChild).toBeNull();
    });

    it("should reveal on user touch scroll and hide after idle", () => {
        const ref = makeRef(800);
        render(<MobileTurnScrubber turns={TURNS} scrollContainerRef={ref} onSelect={vi.fn()} />);
        const track = screen.getByRole("slider", { hidden: true });
        expect(track.getAttribute("aria-hidden")).toBe("true");

        act(() => {
            ref.current!.dispatchEvent(new Event("touchmove"));
        });
        expect(screen.getByRole("slider").getAttribute("aria-hidden")).toBe("false");

        act(() => {
            vi.advanceTimersByTime(2000);
        });
        expect(screen.getByRole("slider", { hidden: true }).getAttribute("aria-hidden")).toBe("true");
    });

    it("should preview the dragged turn and jump to it on release", () => {
        const onSelect = vi.fn();
        const ref = makeRef(800);
        render(<MobileTurnScrubber turns={TURNS} scrollContainerRef={ref} onSelect={onSelect} />);
        const track = screen.getByRole("slider", { hidden: true });
        // 3 轮：轨道高 (3-1) × 18px = 36px，轨道顶部位于 y=100
        const inner = track.firstElementChild as HTMLElement;
        vi.spyOn(inner, "getBoundingClientRect").mockReturnValue({ top: 100 } as DOMRect);

        fireEvent.pointerDown(track, { clientY: 100, pointerId: 1 });
        expect(screen.getByText("第一个问题")).toBeTruthy();

        fireEvent.pointerMove(track, { clientY: 154, pointerId: 1 });
        expect(screen.getByText("第三个问题")).toBeTruthy();
        expect(onSelect).not.toHaveBeenCalled();

        fireEvent.pointerUp(track, { clientY: 154, pointerId: 1 });
        expect(onSelect).toHaveBeenCalledWith(33);
        expect(screen.queryByText("第三个问题")).toBeNull();
    });

    it("should keep one tick per turn for long conversations", () => {
        const turns = Array.from({ length: 40 }, (_, index) => ({ id: index + 1, preview: `q${index}` }));
        const { container } = render(
            <MobileTurnScrubber turns={turns} scrollContainerRef={makeRef(800)} onSelect={vi.fn()} />,
        );
        expect(container.querySelectorAll("[data-turn-tick]")).toHaveLength(40);
    });

    it("should not jump when the drag is cancelled", () => {
        const onSelect = vi.fn();
        render(<MobileTurnScrubber turns={TURNS} scrollContainerRef={makeRef(800)} onSelect={onSelect} />);
        const track = screen.getByRole("slider", { hidden: true });

        fireEvent.pointerDown(track, { clientY: 0, pointerId: 1 });
        fireEvent.pointerCancel(track, { pointerId: 1 });
        expect(onSelect).not.toHaveBeenCalled();
    });
});
