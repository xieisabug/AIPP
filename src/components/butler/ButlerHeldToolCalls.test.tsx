import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { noteToolReview, resetToolReviewsForTests } from "@/hooks/toolReviewStore";
import ButlerHeldToolCalls, { HELD_TOOL_CALL_DELAY_MS } from "./ButlerHeldToolCalls";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("@/components/McpToolCall", () => ({
    default: ({ callId, toolName }: { callId: number; toolName: string }) =>
        <div data-testid={`held-${callId}`}>{toolName}</div>,
}));

const call = (call_id: number, status: MCPToolCallUpdateEvent["status"], conversation_id = 7): MCPToolCallUpdateEvent => ({
    call_id, conversation_id, status, server_name: "aipp-operation", tool_name: `tool_${call_id}`, parameters: "{}",
});
const states = (...calls: MCPToolCallUpdateEvent[]) => new Map(calls.map((c) => [c.call_id, c]));

describe("ButlerHeldToolCalls", () => {
    beforeEach(() => { vi.useFakeTimers(); resetToolReviewsForTests(); });
    afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

    it("shows calls held for confirmation after the settle delay and hides auto-handled ones", () => {
        noteToolReview(2, { phase: "reviewing", callId: 2 });
        noteToolReview(3, { phase: "done", callId: 3, verdict: "safe", reason: "" });
        noteToolReview(4, { phase: "done", callId: 4, verdict: "risky", reason: "删除文件" });
        render(<ButlerHeldToolCalls conversationId={7}
            mcpToolCallStates={states(call(1, "pending"), call(2, "pending"), call(3, "pending"), call(4, "pending"),
                call(5, "executing"), call(6, "pending", 8))} />);
        expect(screen.queryByTestId("held-1")).toBeNull();
        act(() => { vi.advanceTimersByTime(HELD_TOOL_CALL_DELAY_MS); });
        expect(screen.getByTestId("held-1")).toBeTruthy();
        expect(screen.getByTestId("held-4")).toBeTruthy();
        for (const id of [2, 3, 5, 6]) expect(screen.queryByTestId(`held-${id}`)).toBeNull();
    });

    it("shows a call once review rejects it and removes it after the user decides", () => {
        noteToolReview(1, { phase: "reviewing", callId: 1 });
        const { rerender } = render(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"))} />);
        act(() => { vi.advanceTimersByTime(HELD_TOOL_CALL_DELAY_MS * 2); });
        expect(screen.queryByTestId("held-1")).toBeNull();
        act(() => { noteToolReview(1, { phase: "done", callId: 1, verdict: "error", reason: "超时" }); });
        act(() => { vi.advanceTimersByTime(HELD_TOOL_CALL_DELAY_MS); });
        expect(screen.getByTestId("held-1")).toBeTruthy();
        rerender(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "executing"))} />);
        expect(screen.queryByTestId("held-1")).toBeNull();
    });

    it("docks above the floating desktop input capsule instead of sitting under it", () => {
        vi.stubGlobal("ResizeObserver", class { observe() {} disconnect() {} });
        const rect = (left: number, top: number, right: number, bottom: number) =>
            ({ left, top, right, bottom, width: right - left, height: bottom - top, x: left, y: top, toJSON: () => ({}) }) as DOMRect;
        const { container } = render(
            <div data-testid="column">
                <ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"))} floating />
                <div data-aipp-slot="chat-input-area-container" />
            </div>,
        );
        const panel = container.querySelector<HTMLElement>('[data-aipp-slot="butler-held-tool-calls"]')!;
        const anchor = panel.parentElement!;
        const capsule = container.querySelector<HTMLElement>('[data-aipp-slot="chat-input-area-container"]')!;
        anchor.getBoundingClientRect = () => rect(0, 800, 1000, 800);
        capsule.getBoundingClientRect = () => rect(100, 700, 830, 755);
        act(() => { window.dispatchEvent(new Event("resize")); });
        expect(panel.style.left).toBe("100px");
        expect(panel.style.right).toBe("170px");
        expect(panel.style.bottom).toBe("108px");
        act(() => { vi.advanceTimersByTime(HELD_TOOL_CALL_DELAY_MS); });
        expect(panel.querySelector('[data-testid="held-1"]')).toBeTruthy();
    });
});
