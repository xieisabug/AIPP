import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

import ConversationTurnRail, { findActiveTurnId, railTrackHeight } from "./ConversationTurnRail";

/** 记录每个 ResizeObserver 的回调，用来模拟“输入区变高、消息区变矮”。 */
const resizeCallbacks: Array<() => void> = [];

class ResizeObserverMock {
    constructor(callback: () => void) {
        resizeCallbacks.push(callback);
    }
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

/** 高度可变的滚动区，用于模拟输入区变高把消息区挤矮。 */
function makeResizableRef(height: number) {
    let current = height;
    const el = document.createElement("div");
    Object.defineProperty(el, "clientHeight", { get: () => current, configurable: true });
    return {
        ref: { current: el } as React.RefObject<HTMLDivElement | null>,
        setHeight: (next: number) => {
            current = next;
        },
    };
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
        resizeCallbacks.length = 0;
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

    it("should cap the track height and keep one tick per turn in very long conversations", () => {
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
        // 每一轮都有自己的刻度：只变薄，不漏轮
        const ticks = container.querySelectorAll<HTMLElement>("[data-turn-tick]");
        expect(ticks).toHaveLength(300);
        expect(ticks[0].style.height).toBe("1px");
        expect(slider).toHaveAttribute("aria-valuemax", "300");
    });

    it("should wait for the scrolling area to mount before drawing ticks", async () => {
        const ref = { current: null } as React.RefObject<HTMLDivElement | null>;
        const { container } = render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={ref} onSelect={vi.fn()} />,
        );
        // 导航条渲染在滚动区之前，同一个 commit 里 ref 还没填上：此时不画，避免所有刻度叠在顶部
        expect(container.querySelector("[data-aipp-slot='chat-turn-rail']")).toBeNull();

        ref.current = makeScrollContainer(800);
        await act(async () => {
            await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        });

        expect(container.querySelectorAll("[data-turn-tick]")).toHaveLength(TURNS.length);
    });

    it("should give every turn its own tick instead of stacking them", () => {
        const { container } = render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={makeRef(800)} onSelect={vi.fn()} />,
        );
        const tops = [...container.querySelectorAll<HTMLElement>("[data-turn-tick]")]
            .map((tick) => tick.style.top);
        // 3 轮 × 16px 间距：0 / 16 / 32，不能都落在 0
        expect(tops).toEqual(["0px", "16px", "32px"]);
    });

    it("should center the track inside the message area", () => {
        render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={makeRef(800)} onSelect={vi.fn()} />,
        );
        const slider = screen.getByRole("slider");
        // 轨道 32px + 上下各 6px 热区，消息区 800px → 垂直居中
        expect(slider.style.height).toBe("44px");
        expect(slider.style.top).toBe("378px");
    });

    it("should not render while the message area height is unknown", () => {
        const { container } = render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={makeRef(0)} onSelect={vi.fn()} />,
        );
        // 高度为 0 时所有刻度会叠在同一 top，宁可先不画
        expect(container.querySelector("[data-aipp-slot='chat-turn-rail']")).toBeNull();
    });

    it("should re-center and re-space ticks when the message area shrinks", () => {
        const { ref, setHeight } = makeResizableRef(800);
        const turns = Array.from({ length: 30 }, (_, index) => ({ id: index + 1, preview: `q${index}` }));
        const { container } = render(
            <ConversationTurnRail turns={turns} scrollContainerRef={ref} onSelect={vi.fn()} />,
        );
        const rail = () => container.querySelector<HTMLElement>("[data-aipp-slot='chat-turn-rail']");
        const lastTickTop = () =>
            [...container.querySelectorAll<HTMLElement>("[data-turn-tick]")].at(-1)?.style.top;

        // 30 轮：轨道封顶 400px，滑轨垂直居中
        expect(rail()?.style.height).toBe("800px");
        expect(lastTickTop()).toBe("400px");
        expect(screen.getByRole("slider").style.top).toBe("194px");

        act(() => {
            setHeight(400);
            // 第一个 ResizeObserver 是测量滚动区的那一个
            resizeCallbacks[0]();
        });

        expect(rail()?.style.height).toBe("400px");
        expect(lastTickTop()).toBe("200px");
        expect(screen.getByRole("slider").style.top).toBe("94px");
    });

    it("should anchor the rail to the scrolling area instead of the whole conversation", () => {
        const ref = makeRef(800);
        Object.defineProperty(ref.current!, "offsetTop", { value: 120, configurable: true });
        const { container } = render(
            <ConversationTurnRail turns={TURNS} scrollContainerRef={ref} onSelect={vi.fn()} />,
        );
        const rail = container.querySelector<HTMLElement>("[data-aipp-slot='chat-turn-rail']");
        expect(rail?.style.top).toBe("120px");
        expect(rail?.style.height).toBe("800px");
    });
});
