import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { expect, it, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import GeneralAppearanceConfig from "./GeneralAppearanceConfig";

it("opens the guide from settings and refreshes the entry when provider or assistant configuration changes", async () => {
    let canOpen = true;
    const openGuide = vi.fn();
    mockInvokeHandler("get_platform", () => "windows");
    mockInvokeHandler("get_all_feature_config", () => []);
    mockInvokeHandler("list_syntect_themes", () => []);
    mockInvokeHandler("get_autostart_state", () => false);
    mockInvokeHandler("get_onboarding_state", () => ({ canOpen }));
    mockInvokeHandler("open_onboarding_window", openGuide);
    render(<GeneralAppearanceConfig />);
    fireEvent.click(await screen.findByRole("button", { name: "打开引导" }));
    expect(openGuide).toHaveBeenCalledTimes(1);

    canOpen = false;
    await act(() => emit("llm_provider_changed"));
    await waitFor(() => expect(screen.queryByText("首次使用引导")).not.toBeInTheDocument());
    canOpen = true;
    await act(() => emit("assistant_list_changed"));
    expect(await screen.findByRole("button", { name: "打开引导" })).toBeVisible();
});
