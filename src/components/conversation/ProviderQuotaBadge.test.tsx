import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ProviderQuotaBadge, { clearProviderQuotaCache } from "./ProviderQuotaBadge";

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

function makeReading(windows: unknown[], checkedAt = Date.now()) {
    return { windows, checkedAt, expiresAt: checkedAt + 15 * 60_000 };
}

describe("ProviderQuotaBadge", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        clearProviderQuotaCache();
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

    it("renders nothing when the conversation does not use a quota-enabled provider", async () => {
        mockInvokeHandler("get_chat_quota_provider", () => null);
        render(<ProviderQuotaBadge conversationId={10} assistantId={2} />);
        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("get_chat_quota_provider", {
                conversationId: 10,
                assistantId: 2,
            }),
        );
        expect(screen.queryByTestId("chat-provider-quota")).toBeNull();
        expect(invoke).not.toHaveBeenCalledWith("get_provider_quota", expect.anything());
    });

    it("renders one concentric ring per limit window and shows details on hover", async () => {
        mockInvokeHandler("get_chat_quota_provider", () => ({
            providerId: 42,
            providerName: "OpenCode Go",
            apiType: "opencode_go",
            label: "订阅额度",
        }));
        mockInvokeHandler("get_provider_quota", () =>
            makeReading([
                { label: "5 小时", remainingPercent: 80, resetsAt: "2026-12-01T08:00:00Z" },
                { label: "每周", remainingPercent: 55.5, resetsAt: null },
                { label: "每月", remainingPercent: 92, resetsAt: null },
            ]),
        );

        render(<ProviderQuotaBadge conversationId={10} assistantId={2} />);
        const badge = await screen.findByTestId("chat-provider-quota");
        expect(badge).toHaveAttribute("data-quota-mode", "rings");
        expect(screen.getAllByTestId("quota-ring-cluster")).toHaveLength(1);
        expect(screen.getAllByTestId("quota-ring")).toHaveLength(3);

        // Hover to open details card
        fireEvent.pointerEnter(badge, { pointerType: "mouse" });
        const popover = await screen.findByTestId("chat-provider-quota-popover");
        expect(popover).toHaveTextContent("OpenCode Go");
        expect(popover).toHaveTextContent("5 小时");
        expect(popover).toHaveTextContent("剩余 80%");
        expect(popover).toHaveTextContent("每周");
        expect(popover).toHaveTextContent("剩余 55.5%");
        expect(popover).toHaveTextContent("每月");
        expect(popover).toHaveTextContent("剩余 92%");
    });

    it("renders amount-only quota in DeepSeek-like pill mode", async () => {
        mockInvokeHandler("get_chat_quota_provider", () => ({
            providerId: 43,
            providerName: "Credits Provider",
            apiType: "cursor",
            label: "订阅额度",
        }));
        mockInvokeHandler("get_provider_quota", () =>
            makeReading([
                { label: "账户额度", remainingPercent: null, remainingAmount: 19.5, currency: "USD", resetsAt: null },
            ]),
        );

        render(<ProviderQuotaBadge conversationId={11} assistantId={3} />);
        const badge = await screen.findByTestId("chat-provider-quota");
        expect(badge).toHaveAttribute("data-quota-mode", "amount");
        expect(badge).toHaveTextContent("$19.50");
        expect(screen.queryByTestId("quota-ring")).toBeNull();
    });

    it("forces refresh on click and after a conversation turn completely finishes", async () => {
        let percent = 80;
        mockInvokeHandler("get_chat_quota_provider", () => ({
            providerId: 44,
            providerName: "Codex",
            apiType: "codex_app_server",
            label: "订阅额度",
        }));
        mockInvokeHandler("get_provider_quota", () =>
            makeReading([
                { label: "codex · 5 小时", remainingPercent: percent, resetsAt: null },
                { label: "codex · 每周", remainingPercent: 90, resetsAt: null },
            ]),
        );

        render(<ProviderQuotaBadge conversationId={12} assistantId={4} settleDelayMs={25} />);
        const badge = await screen.findByTestId("chat-provider-quota");
        await waitFor(() => expect(screen.getAllByTestId("quota-ring")).toHaveLength(2));
        expect(invoke).toHaveBeenCalledWith("get_provider_quota", { providerId: 44, force: false });

        // Click forces refresh
        percent = 70;
        fireEvent.click(badge);
        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("get_provider_quota", { providerId: 44, force: true }),
        );
        await waitFor(() =>
            expect(screen.getAllByTestId("quota-ring")[0]).toHaveAttribute("data-ring-percent", "70"),
        );

        // Turn running -> brief idle -> resume should not refresh prematurely
        percent = 60;
        await emitRuntime(12, true, "user_pending");
        await emitRuntime(12, false, "idle");
        await emitRuntime(12, true, "assistant_streaming");
        await act(async () => {
            await new Promise((resolve) => setTimeout(resolve, 50));
        });
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "get_provider_quota")).toHaveLength(2);

        // Turn fully ends -> forced refresh after settle
        await emitRuntime(12, false, "idle");
        await waitFor(() =>
            expect(screen.getAllByTestId("quota-ring")[0]).toHaveAttribute("data-ring-percent", "60"),
        );
        expect(invoke.mock.calls.filter(([cmd]) => cmd === "get_provider_quota")).toHaveLength(3);
    });

    it("hides immediately when the setting toggle is turned off", async () => {
        let enabled = true;
        mockInvokeHandler("get_chat_quota_provider", () =>
            enabled
                ? { providerId: 45, providerName: "Cursor", apiType: "cursor", label: "订阅额度" }
                : null,
        );
        mockInvokeHandler("get_provider_quota", () =>
            makeReading([{ label: "本账期 · 总额度", remainingPercent: 50, resetsAt: null }]),
        );

        render(<ProviderQuotaBadge conversationId={15} assistantId={5} />);
        expect(await screen.findByTestId("chat-provider-quota")).toBeInTheDocument();

        enabled = false;
        await act(async () => {
            await emit("llm_provider_changed", null);
            await Promise.resolve();
        });
        await waitFor(() => expect(screen.queryByTestId("chat-provider-quota")).toBeNull());
    });
});
