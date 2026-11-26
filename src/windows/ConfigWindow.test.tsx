import { StrictMode } from "react";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit, listen } from "@tauri-apps/api/event";
import { afterEach, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { registerConfigBeforeLeave } from "@/lib/configLeaveGuard";
import ConfigWindow from "./ConfigWindow";

vi.mock("@tauri-apps/api/webviewWindow", () => ({
    getCurrentWebviewWindow: () => ({ label: "config", listen }),
}));
vi.mock("@/hooks/useTheme", () => ({ useTheme: () => {} }));
vi.mock("@/hooks/use-mobile", () => ({ useIsMobile: () => false }));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => "linux", useIsMobilePlatform: () => false }));
vi.mock("@/services/PluginRuntime", () => ({ pluginRuntime: { loadPlugins: async () => [] } }));
vi.mock("@/components/config/SettingsSearch", () => ({ default: () => null }));
vi.mock("@/components/config/LLMProviderConfig", () => ({ default: () => null }));
vi.mock("@/components/config/AssistantConfig", () => ({
    default: ({ subNav, onSubNavConsumed }: { subNav?: string; onSubNavConsumed: () => void }) =>
        <button onClick={onSubNavConsumed}>{subNav ?? "助手列表"}</button>,
}));

afterEach(clearAllMockHandlers);

it("should deliver creation requests on startup and while open through the settings leave guard", async () => {
    const create = { menu: "assistant-config", subNav: "create" };
    let pending: typeof create | null = create;
    mockInvokeHandler("take_config_navigation", () => {
        const target = pending;
        pending = null;
        return target;
    });
    render(<StrictMode><ConfigWindow /></StrictMode>);
    fireEvent.click(await screen.findByRole("button", { name: "create" }));
    expect(screen.getByRole("button", { name: "助手列表" })).toBeVisible();

    const leaveGuard = vi.fn<(proceed: () => void) => void>();
    const unregister = registerConfigBeforeLeave(leaveGuard);
    try {
        pending = create;
        await act(async () => { await emit("config-navigation-pending"); });
        await waitFor(() => expect(leaveGuard).toHaveBeenCalledTimes(1));
        expect(screen.queryByRole("button", { name: "create" })).toBeNull();
        act(() => leaveGuard.mock.calls[0][0]());
        fireEvent.click(screen.getByRole("button", { name: "create" }));
        await act(async () => { await emit("config-navigation-pending"); });
        expect(leaveGuard).toHaveBeenCalledTimes(1);
        expect(screen.getByRole("button", { name: "助手列表" })).toBeVisible();
    } finally {
        unregister();
    }
});
