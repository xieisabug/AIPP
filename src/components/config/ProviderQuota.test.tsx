import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it } from "vitest";
import { invoke, mockInvokeHandler } from "../../__tests__/mocks/tauri";
import { ProviderQuota } from "./ProviderQuota";

it("should show the selected provider's windows and report a failed refresh", async () => {
    mockInvokeHandler("get_provider_quota", () => [
        { label: "5 小时", remainingPercent: 0, resetsAt: "2026-12-01T08:00:00Z" },
        { label: "每周", remainingPercent: 88.75, resetsAt: null },
    ]);
    const { container } = render(<ProviderQuota providerId={42} />);
    expect(await screen.findByText("剩余 0%", {}, { timeout: 3000 })).toBeVisible();
    expect(screen.getByText("剩余 88.75%")).toBeVisible();
    expect(container.querySelector("time")).toHaveAttribute("datetime", "2026-12-01T08:00:00Z");
    expect(screen.getByText("服务未提供重置时间")).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("get_provider_quota", { providerId: 42 });
    mockInvokeHandler("get_provider_quota", () => { throw new Error("HTTP 403"); });
    fireEvent.click(screen.getByRole("button", { name: "刷新订阅额度" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("HTTP 403");
    expect(screen.queryByText("剩余 0%")).toBeNull();
});
