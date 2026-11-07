import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { noteToolReview, resetToolReviewsForTests } from "@/hooks/toolReviewStore";
import ButlerHeldToolCalls, { HELD_TOOL_CALL_DELAY_MS, REVIEWING_NOTICE_DELAY_MS, describeHold, summarizeParameters } from "./ButlerHeldToolCalls";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(() => Promise.resolve(null)) }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));
const antiLeakage = { enabled: false, isRevealed: false };
vi.mock("@/contexts/AntiLeakageContext", () => ({ useAntiLeakage: () => antiLeakage }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));

const call = (call_id: number, status: MCPToolCallUpdateEvent["status"], conversation_id = 7, parameters = "{}"): MCPToolCallUpdateEvent => ({
    call_id, conversation_id, status, server_name: "aipp-operation", tool_name: `tool_${call_id}`, parameters,
});
const states = (...calls: MCPToolCallUpdateEvent[]) => new Map(calls.map((c) => [c.call_id, c]));
const heldNames = () => screen.queryAllByRole("group").map((el) => el.getAttribute("aria-label"));
const settle = () => act(() => { vi.advanceTimersByTime(HELD_TOOL_CALL_DELAY_MS); });

describe("ButlerHeldToolCalls", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        resetToolReviewsForTests();
        vi.mocked(invoke).mockReset().mockImplementation(() => Promise.resolve(null));
        vi.mocked(toast.error).mockReset();
        antiLeakage.enabled = false;
    });
    afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

    it("should show held and reviewing calls but hide auto-approved or settled ones", () => {
        noteToolReview(2, { phase: "reviewing", callId: 2 });
        noteToolReview(3, { phase: "done", callId: 3, verdict: "safe", reason: "" });
        noteToolReview(4, { phase: "done", callId: 4, verdict: "risky", reason: "删除文件" });
        render(<ButlerHeldToolCalls conversationId={7}
            mcpToolCallStates={states(call(1, "pending"), call(2, "pending"), call(3, "pending"), call(4, "pending"),
                call(5, "executing"), call(6, "pending", 8))} />);
        // 审核结论已出（有风险）立即显示；审核中短暂延迟后显示；未知状态等满延迟。
        expect(heldNames()).toEqual(["待确认的工具调用 tool_4"]);
        settle();
        expect(heldNames()).toEqual(["待确认的工具调用 tool_1", "正在自动审核的工具调用 tool_2", "待确认的工具调用 tool_4"]);
        expect(screen.getByText("自动审核认为有风险")).toBeTruthy();
        expect(screen.getByText("：删除文件")).toBeTruthy();
        expect(screen.getByText("需要你确认后执行")).toBeTruthy();
    });

    it("should show review progress and switch the same card to confirmation when review fails", () => {
        noteToolReview(1, { phase: "reviewing", callId: 1 });
        const { rerender } = render(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"))} />);
        expect(heldNames()).toEqual([]);
        act(() => { vi.advanceTimersByTime(REVIEWING_NOTICE_DELAY_MS); });
        expect(heldNames()).toEqual(["正在自动审核的工具调用 tool_1"]);
        expect(screen.getByText("正在自动审核")).toBeTruthy();
        expect(screen.getByText("，通过后会自动执行，无需操作")).toBeTruthy();
        fireEvent.click(screen.getByText("参数"));
        act(() => { noteToolReview(1, { phase: "done", callId: 1, verdict: "error", reason: "超时" }); });
        // 原地切换，不先消失再出现；展开状态保留。
        expect(heldNames()).toEqual(["待确认的工具调用 tool_1"]);
        expect(screen.getByText("自动审核失败，需要你确认")).toBeTruthy();
        expect(screen.getByText("收起参数")).toBeTruthy();
        expect(screen.getByText("允许执行")).toBeTruthy();
        rerender(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "executing"))} />);
        expect(heldNames()).toEqual([]);
    });

    it("should hide the reviewing card once review approves the call", () => {
        noteToolReview(1, { phase: "reviewing", callId: 1 });
        render(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"))} />);
        act(() => { vi.advanceTimersByTime(REVIEWING_NOTICE_DELAY_MS); });
        expect(heldNames()).toEqual(["正在自动审核的工具调用 tool_1"]);
        act(() => { noteToolReview(1, { phase: "done", callId: 1, verdict: "safe", reason: "" }); });
        expect(heldNames()).toEqual([]);
    });

    it("should let the user skip the running review and execute directly", async () => {
        noteToolReview(1, { phase: "reviewing", callId: 1 });
        render(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"))} />);
        act(() => { vi.advanceTimersByTime(REVIEWING_NOTICE_DELAY_MS); });
        await act(async () => { fireEvent.click(screen.getByText("跳过审核执行")); });
        expect(invoke).toHaveBeenCalledWith("execute_mcp_tool_call", { callId: 1, triggerContinuation: true });
    });

    it("should execute or reject the call when the user clicks the action buttons", async () => {
        render(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"), call(2, "pending"))} />);
        settle();
        await act(async () => { fireEvent.click(screen.getAllByText("允许执行")[0]); });
        expect(invoke).toHaveBeenCalledWith("execute_mcp_tool_call", { callId: 1, triggerContinuation: true });
        await act(async () => { fireEvent.click(screen.getAllByText("拒绝")[1]); });
        expect(invoke).toHaveBeenCalledWith("reject_mcp_tool_call", { callId: 2 });
    });

    it("should disable actions while a decision is in flight and toast the concrete error on failure", async () => {
        let fail: (reason: unknown) => void = () => undefined;
        vi.mocked(invoke).mockImplementation(() => new Promise((_, reject) => { fail = reject; }));
        render(<ButlerHeldToolCalls conversationId={7} mcpToolCallStates={states(call(1, "pending"))} />);
        settle();
        fireEvent.click(screen.getByText("允许执行"));
        const buttons = screen.getByRole("group").querySelectorAll<HTMLButtonElement>("button:disabled");
        expect(buttons.length).toBe(2);
        fireEvent.click(screen.getByText("拒绝"));
        const decisions = vi.mocked(invoke).mock.calls.filter(([cmd]) => cmd !== "get_tool_review");
        expect(decisions.map(([cmd]) => cmd)).toEqual(["execute_mcp_tool_call"]);
        await act(async () => { fail("MCP 服务未连接"); });
        expect(toast.error).toHaveBeenCalledWith("执行工具 tool_1 失败（调用 #1）：MCP 服务未连接");
        expect(screen.getByText("允许执行").closest("button")!.disabled).toBe(false);
    });

    it("should summarize parameters when collapsed and show full JSON when expanded", () => {
        render(<ButlerHeldToolCalls conversationId={7}
            mcpToolCallStates={states(call(1, "pending", 7, '{"path":"/tmp/a","recursive":true}'))} />);
        settle();
        expect(screen.getByText("path: /tmp/a · recursive: true")).toBeTruthy();
        fireEvent.click(screen.getByText("参数"));
        expect(screen.getByText("收起参数")).toBeTruthy();
        expect(screen.getByText(/"recursive": true/)).toBeTruthy();
    });

    it("should mask tool name and parameters when anti-leakage is enabled", () => {
        antiLeakage.enabled = true;
        render(<ButlerHeldToolCalls conversationId={7}
            mcpToolCallStates={states(call(1, "pending", 7, '{"secret":"abc"}'))} />);
        settle();
        expect(screen.queryByText(/abc/)).toBeNull();
        expect(screen.queryByText("tool_1")).toBeNull();
    });

    it("should dock above the floating desktop input capsule instead of sitting under it", () => {
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
        settle();
        expect(panel.querySelector('[data-aipp-slot="butler-held-tool-call"]')).toBeTruthy();
    });
});

describe("held tool call helpers", () => {
    it("should describe why a call is held based on the review verdict", () => {
        expect(describeHold(null)).toEqual({ title: "需要你确认后执行", reason: "" });
        expect(describeHold({ phase: "done", callId: 1, verdict: "risky", reason: " 目标不明确 " }))
            .toEqual({ title: "自动审核认为有风险", reason: "目标不明确" });
        expect(describeHold({ phase: "done", callId: 1, verdict: "error", reason: "超时" }).title).toBe("自动审核失败，需要你确认");
    });

    it("should truncate long values and fall back to raw text when parameters are not JSON", () => {
        expect(summarizeParameters(JSON.stringify({ text: "x".repeat(60) }))).toBe(`text: ${"x".repeat(40)}…`);
        expect(summarizeParameters("not json")).toBe("not json");
        expect(summarizeParameters("{}")).toBe("");
    });
});
