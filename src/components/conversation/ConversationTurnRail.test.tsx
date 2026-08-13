import React from "react";
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi, beforeEach, afterEach } from "vitest";

import ConversationTurnRail, { findActiveTurnId } from "./ConversationTurnRail";

// Tooltip 用直通渲染，避免 Radix Portal 在 happy-dom 中的时序问题
vi.mock("@/components/ui/tooltip", () => ({
    Tooltip: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    TooltipTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    TooltipContent: ({ children }: { children: React.ReactNode }) => (
        <div data-testid="tooltip-content">{children}</div>
    ),
    TooltipProvider: ({ children }: { children: React.ReactNode }) => <>{children}</>,
}));

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

    it("renders one button per turn and invokes onSelect on click", () => {
        const onSelect = vi.fn();
        render(
            <ConversationTurnRail
                turns={[
                    { id: 101, preview: "first question" },
                    { id: 202, preview: "second question" },
                ]}
                scrollContainerRef={makeRef(800)}
                onSelect={onSelect}
            />,
        );

        const buttons = screen.getAllByRole("button");
        expect(buttons).toHaveLength(2);
        expect(buttons[0]).toHaveAttribute("aria-label", "跳转到第 1 轮对话");
        expect(buttons[1]).toHaveAttribute("aria-label", "跳转到第 2 轮对话");

        fireEvent.click(buttons[0]);
        expect(onSelect).toHaveBeenCalledWith(101);
        fireEvent.click(buttons[1]);
        expect(onSelect).toHaveBeenCalledWith(202);
    });

    it("marks clicked turn as current", () => {
        render(
            <ConversationTurnRail
                turns={[
                    { id: 101, preview: "first question" },
                    { id: 202, preview: "second question" },
                ]}
                scrollContainerRef={makeRef(800)}
                onSelect={vi.fn()}
            />,
        );

        const buttons = screen.getAllByRole("button");
        fireEvent.click(buttons[1]);

        expect(buttons[0]).not.toHaveAttribute("aria-current");
        expect(buttons[1]).toHaveAttribute("aria-current", "true");
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

    it("uses the max gap when there is plenty of vertical space", () => {
        render(
            <ConversationTurnRail
                turns={Array.from({ length: 5 }, (_, i) => ({
                    id: i + 1,
                    preview: `q${i}`,
                }))}
                scrollContainerRef={makeRef(2000)}
                onSelect={vi.fn()}
            />,
        );

        const button = screen.getAllByRole("button")[0];
        const wrapper = button.parentElement as HTMLElement;
        // BAR_H=6, GAP_MAX=10: 5*6 + 4*10 = 70, available = 2000*0.7 = 1400 → plenty
        expect(wrapper.style.gap).toBe("10px");
    });

    it("shrinks the gap below the max when the container is short", () => {
        render(
            <ConversationTurnRail
                turns={Array.from({ length: 20 }, (_, i) => ({
                    id: i + 1,
                    preview: `q${i}`,
                }))}
                scrollContainerRef={makeRef(50)}
                onSelect={vi.fn()}
            />,
        );

        const button = screen.getAllByRole("button")[0];
        const wrapper = button.parentElement as HTMLElement;
        // needed = 20*6 + 19*10 = 310 >> available = 50*0.7 = 35 → clamped to GAP_MIN=2
        expect(wrapper.style.gap).toBe("2px");
    });
});
