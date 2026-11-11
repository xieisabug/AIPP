import React from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

import ConversationTurnRail, { findActiveTurnId, railTrackHeight } from "./ConversationTurnRail";

class ResizeObserverMock {
    observe = vi.fn();
    disconnect = vi.fn();
}

function makeScrollContainer(height: number) {
    const el = document.createElement("div");
    Object.defineProperty(el, "clientHeight", {
        get: () => height,
        configurable: true,
    });
    return el;
}

function makeRef(height: number) {
    return {
        current: makeScrollContainer(height),
    } as React.RefObject<HTMLDivElement | null>;
}

const TURNS = [
    { id: 101, preview: "first question" },
    { id: 202, preview: "second question" },
    { id: 303, preview: "third question" },
];

function mockTrackTop(slider: HTMLElement, top: number) {
    const track = slider.firstElementChild as HTMLElement;
    vi.spyOn(track, "getBoundingClientRect").mockReturnValue({ top } as DOMRect);
}

describe("ConversationTurnRail", () => {
    beforeEach(() => {
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    it("renders nothing when turns is empty", () => {
        const { container } = render(
            <ConversationTurnRail
                turns={[]}
                scrollContainerRef={makeRef(800)}
                onSelect={vi.fn()}
            />,
        );
        expect(container.firstChild).toBeNull();
    });

    it("should jump to the nearest turn when the track is clicked", () => {
        const onSelect = vi.fn();
        render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={makeRef(800)} onSelect={onSelect} />,
        );
        const slider = screen.getByRole("slider");
        // 3 轮 × 16px 间距 = 32px 轨道，轨道顶部位于 y=100
        mockTrackTop(slider, 100);

        fireEvent.click(slider, { clientY: 100 });
        expect(onSelect).toHaveBeenLastCalledWith(101);
        fireEvent.click(slider, { clientY: 133 });
        expect(onSelect).toHaveBeenLastCalledWith(303);
        expect(slider).toHaveAttribute("aria-valuenow", "3");
    });

    it("should preview the hovered turn and hide the preview on leave", () => {
        render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={makeRef(800)} onSelect={vi.fn()} />,
        );
        const slider = screen.getByRole("slider");
        mockTrackTop(slider, 100);

        fireEvent.pointerMove(slider, { clientY: 117 });
        expect(screen.getByText("第 2 / 3 轮")).toBeTruthy();
        expect(screen.getByText("second question")).toBeTruthy();

        fireEvent.pointerLeave(slider);
        expect(screen.queryByText("second question")).toBeNull();
    });

    it("should step through turns with the keyboard and jump on Enter", () => {
        const onSelect = vi.fn();
        render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={makeRef(800)} onSelect={onSelect} />,
        );
        const slider = screen.getByRole("slider");

        fireEvent.keyDown(slider, { key: "ArrowDown" });
        expect(screen.getByText("second question")).toBeTruthy();
        fireEvent.keyDown(slider, { key: "End" });
        expect(screen.getByText("third question")).toBeTruthy();
        expect(onSelect).not.toHaveBeenCalled();

        fireEvent.keyDown(slider, { key: "Enter" });
        expect(onSelect).toHaveBeenCalledWith(303);
        expect(screen.queryByText("third question")).toBeNull();
    });

    it("keeps a user turn active until the next user turn boundary", () => {
        const turns = [{ id: 101 }, { id: 202 }, { id: 303 }];
        const positions = new Map([
            [101, 100],
            [202, 500],
            [303, 900],
        ]);

        expect(findActiveTurnId(turns, positions, 120)).toBe(101);
        expect(findActiveTurnId(turns, positions, 443)).toBe(101);
        expect(findActiveTurnId(turns, positions, 444)).toBe(202);
        expect(findActiveTurnId(turns, positions, 860)).toBe(303);
    });

    it("should size the track per turn while there is room", () => {
        expect(railTrackHeight(1, 800)).toBe(0);
        expect(railTrackHeight(5, 2000)).toBe(64);
    });

    it("should cap the track height and sample ticks for very long conversations", () => {
        expect(railTrackHeight(300, 800)).toBe(400);

        const { container } = render(
            <ConversationTurnRail
                turns={Array.from({ length: 300 }, (_, i) => ({ id: i + 1, preview: `q${i}` }))}
                scrollContainerRef={makeRef(800)}
                onSelect={vi.fn()}
            />,
        );

        const slider = screen.getByRole("slider");
        const track = slider.firstElementChild as HTMLElement;
        expect(track.style.height).toBe("400px");
        const ticks = container.querySelectorAll("[data-turn-tick]");
        expect(ticks.length).toBeGreaterThan(2);
        expect(ticks.length).toBeLessThanOrEqual(400 / 8 + 1);
        expect(slider).toHaveAttribute("aria-valuemax", "300");
    });
});
