import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import DeepSeekBalanceBadge, { clearDeepSeekBalanceCache } from "./DeepSeekBalanceBadge";

async function emitRuntime(conversationId: number, isRunning: boolean, phase = isRunning ? "assistant_streaming" : "idle") {
    await act(async () => {
        await emit(`conversation_event_${conversationId}`, {
            type: "runtime_state_snapshot",
            data: {
                state: {
                    conversation_id: conversationId,
                    is_running: isRunning,
                    phase,
                    epoch: 1,
                    revision: 1,
                },
            },
        });
        await Promise.resolve();
    });
}

describe("DeepSeekBalanceBadge", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        clearDeepSeekBalanceCache();
        invoke.mockClear();
        mockInvokeHandler("get_conversation_runtime_state", (args) => ({
            conversation_id: Number(args?.conversationId ?? 0),
            is_running: false,
            phase: "idle",
            epoch: 0,
            revision: 0,
        }));
    });

    afterEach(() => {
        vi.useRealTimers();
    });

    it("renders nothing when the assistant does not use an enabled DeepSeek provider", async () => {
        mockInvokeHandler("get_deepseek_chat_balance_provider", () => null);
        render(<DeepSeekBalanceBadge conversationId={10} assistantId={2} />);
        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("get_deepseek_chat_balance_provider", {
                conversationId: 10,
                assistantId: 2,
            }),
        );
        expect(screen.queryByTestId("chat-deepseek-balance")).toBeNull();
        expect(invoke).not.toHaveBeenCalledWith("get_deepseek_balance", expect.anything());
    });

    it("displays the live balance and refreshes once a turn completely finishes", async () => {
        let balance = { available: true, currency: "CNY", total: "19.00" };
        mockInvokeHandler("get_deepseek_chat_balance_provider", () => 42);
        mockInvokeHandler("get_deepseek_balance", () => balance);

        render(<DeepSeekBalanceBadge conversationId={10} assistantId={2} settleDelayMs={30} />);
        expect(await screen.findByTestId("chat-deepseek-balance")).toHaveTextContent("¥19.00");

        const initialCalls = invoke.mock.calls.filter(([cmd]) => cmd === "get_deepseek_balance").length;
        expect(initialCalls).toBe(1);

        // 对话开始并中途经历了工具调用续跑，不应提前刷新
        balance = { available: true, currency: "CNY", total: "18.85" };
        await emitRuntime(10, true, "user_pending");
        await emitRuntime(10, true, "mcp_executing");
        await emitRuntime(10, false, "idle");
        // 短暂空闲后立即进入下一轮续跑，应取消上一次防抖刷新
        await emitRuntime(10, true, "assistant_streaming");
        await act(async () => {
            await new Promise((resolve) => setTimeout(resolve, 60));
        });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "get_deepseek_balance")).toHaveLength(1);

        // 本轮对话彻底结束 -> 稳定空闲后自动刷新余额
        await emitRuntime(10, false, "idle");
        await waitFor(() => expect(screen.getByTestId("chat-deepseek-balance")).toHaveTextContent("¥18.85"));
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "get_deepseek_balance")).toHaveLength(2);
    });

    it("refreshes when opening a conversation that is already running and finishes later", async () => {
        let balance = { available: true, currency: "CNY", total: "12.50" };
        mockInvokeHandler("get_deepseek_chat_balance_provider", () => 42);
        mockInvokeHandler("get_deepseek_balance", () => balance);
        mockInvokeHandler("get_conversation_runtime_state", () => ({
            conversation_id: 20,
            is_running: true,
            phase: "assistant_streaming",
            epoch: 1,
            revision: 2,
        }));

        render(<DeepSeekBalanceBadge conversationId={20} assistantId={3} settleDelayMs={20} />);
        expect(await screen.findByTestId("chat-deepseek-balance")).toHaveTextContent("¥12.50");

        balance = { available: false, currency: "CNY", total: "0.00" };
        await emitRuntime(20, false, "idle");

        await waitFor(() =>
            expect(screen.getByTestId("chat-deepseek-balance")).toHaveTextContent("¥0.00 余额不足"),
        );
    });

    it("reacts immediately when the provider toggle is turned off or on and supports click refresh", async () => {
        let enabled = true;
        let balance = { available: true, currency: "CNY", total: "9.90" };
        mockInvokeHandler("get_deepseek_chat_balance_provider", () => (enabled ? 42 : null));
        mockInvokeHandler("get_deepseek_balance", () => balance);

        render(<DeepSeekBalanceBadge conversationId={30} assistantId={4} settleDelayMs={20} />);
        expect(await screen.findByTestId("chat-deepseek-balance")).toHaveTextContent("¥9.90");

        // 点击徽章手动刷新
        balance = { available: true, currency: "CNY", total: "9.80" };
        fireEvent.click(screen.getByTestId("chat-deepseek-balance"));
        await waitFor(() => expect(screen.getByTestId("chat-deepseek-balance")).toHaveTextContent("¥9.80"));

        // 在设置里关闭开关后立即隐藏
        enabled = false;
        await act(async () => {
            await emit("llm_provider_changed", null);
            await Promise.resolve();
        });
        await waitFor(() => expect(screen.queryByTestId("chat-deepseek-balance")).toBeNull());
    });
});
