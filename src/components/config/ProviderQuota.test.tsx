import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { invoke, mockInvokeHandler } from "../../__tests__/mocks/tauri";
import { ProviderQuota } from "./ProviderQuota";

function reading(windows: unknown[], checkedAt = Date.now()) {
    return { windows, checkedAt, expiresAt: checkedAt + 15 * 60_000 };
}

it("should show the selected provider's windows and report a failed refresh", async () => {
    mockInvokeHandler("get_provider_quota", () => reading([
        { label: "5 小时", remainingPercent: 0, resetsAt: "2026-12-01T08:00:00Z" },
        { label: "每周", remainingPercent: 88.75, resetsAt: null },
    ]));
    const { container } = render(<ProviderQuota providerId={42} />);
    expect(await screen.findByText("剩余 0%", {}, { timeout: 3000 })).toBeVisible();
    expect(screen.getByText("剩余 88.75%")).toBeVisible();
    expect(container.querySelector("time")).toHaveAttribute("datetime", "2026-12-01T08:00:00Z");
    expect(screen.getByText("服务未提供重置时间")).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("get_provider_quota", { providerId: 42, force: false });
    mockInvokeHandler("get_provider_quota", () => { throw new Error("HTTP 403"); });
    fireEvent.click(screen.getByRole("button", { name: "刷新订阅额度" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("HTTP 403");
    expect(invoke).toHaveBeenCalledWith("get_provider_quota", { providerId: 42, force: true });
    expect(screen.getByText("剩余 0%")).toBeVisible();
});

it("should show each local CLI model group's windows under its own heading", async () => {
    mockInvokeHandler("get_provider_quota", () => reading([
        { group: "Gemini Models", label: "每周", remainingPercent: 37.5, resetsAt: "2026-12-01T02:00:00Z" },
        { group: "Gemini Models", label: "5 小时", remainingPercent: 0, resetsAt: null },
        { group: "Claude and GPT models", label: "每周", remainingPercent: null, remainingAmount: 12, resetsAt: null },
    ]));
    render(<ProviderQuota providerId={42} label="本机 Antigravity CLI 额度" />);
    const gemini = await screen.findByRole("region", { name: "Gemini Models" }, { timeout: 3000 });
    expect(within(gemini).getByText("每周")).toBeVisible();
    expect(within(gemini).getByText("剩余 37.5%")).toBeVisible();
    expect(within(gemini).getByText("5 小时")).toBeVisible();
    expect(within(gemini).getByText("剩余 0%")).toBeVisible();
    expect(gemini.querySelector("time")).toHaveAttribute("datetime", "2026-12-01T02:00:00Z");
    const other = within(screen.getByRole("region", { name: "Claude and GPT models" }));
    expect(other.getByText("每周")).toBeVisible();
    expect(other.getByText("剩余 12")).toBeVisible();
    expect(screen.getByRole("button", { name: "刷新本机 Antigravity CLI 额度" })).toBeEnabled();
});

it("should refresh at the original cache expiry and restart the timer after a manual refresh", async () => {
    vi.useFakeTimers();
    const minute = 60_000;
    const initialTime = Date.now();
    let calls = 0;
    mockInvokeHandler("get_provider_quota", () => {
        calls++;
        return reading([], calls === 1 ? initialTime - 12 * minute : Date.now());
    });
    const advance = (ms: number) => act(() => vi.advanceTimersByTimeAsync(ms));
    const view = render(<ProviderQuota providerId={42} />);
    try {
        await advance(1000);
        expect(screen.getByRole("status")).toHaveTextContent(new Date(initialTime - 12 * minute).toLocaleTimeString());
        await advance(3 * minute - 1001);
        expect(calls).toBe(1);
        await advance(101);
        expect(calls).toBe(2);
        await advance(minute);
        fireEvent.click(screen.getByRole("button", { name: "刷新订阅额度" }));
        await advance(101);
        expect(calls).toBe(3);
        await advance(14 * minute);
        expect(calls).toBe(3);
        await advance(minute + 101);
        expect(calls).toBe(4);
        expect(invoke).toHaveBeenLastCalledWith("get_provider_quota", { providerId: 42, force: false });
    } finally { view.unmount(); vi.useRealTimers(); }
});
