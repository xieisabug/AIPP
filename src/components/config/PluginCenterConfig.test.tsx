import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { pluginRuntime, type LoadedPlugin } from "@/services/PluginRuntime";
import PluginCenterConfig from "./PluginCenterConfig";

vi.mock("@/services/PluginRuntime", () => ({ pluginRuntime: { reloadPlugins: vi.fn() } }));

const plugin: LoadedPlugin = {
    pluginId: 1, name: "朱砂", code: "theme", version: "1", pluginType: ["themeType"], instance: null,
};
const installed = { ...plugin, isActive: true, isInstalled: true };

describe("插件中心运行状态", () => {
    beforeEach(() => {
        mockInvokeHandler("get_platform", () => "macos");
        mockInvokeHandler("list_plugins", () => [installed]);
        mockInvokeHandler("get_plugin_detail", () => null);
        vi.stubGlobal("innerWidth", 1440);
        for (const cmd of ["get_plugin_config", "get_plugin_hook_registrations", "list_plugin_hook_audit_logs"]) {
            mockInvokeHandler(cmd, () => []);
        }
    });

    it("should show a load failure without a view and clear it after retry", async () => {
        const reload = vi.mocked(pluginRuntime.reloadPlugins);
        reload.mockResolvedValueOnce([{ ...plugin, loadError: "WebView 未能加载插件脚本" }]);
        reload.mockResolvedValueOnce([{ ...plugin, instance: {} as AippPlugin }]);
        render(<PluginCenterConfig pluginList={[]} />);
        expect(await screen.findByRole("alert")).toHaveTextContent("WebView 未能加载插件脚本");
        expect(screen.getByText("朱砂（加载失败）")).toBeVisible();
        expect(screen.queryByText("已启用")).toBeNull();
        expect(screen.queryByRole("button", { name: "打开插件文件夹" })).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "重试加载" }));
        await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
        expect(screen.getByText("已启用")).toBeVisible();
    });

    it("should guide the user to file permissions when the OS denies directory access", async () => {
        mockInvokeHandler("list_plugins", () => [{ ...installed, entryAccessError: "/theme/plugin.json: Permission denied" }]);
        vi.mocked(pluginRuntime.reloadPlugins).mockResolvedValue([]);
        render(<PluginCenterConfig pluginList={[]} />);
        expect(await screen.findByRole("alert")).toHaveTextContent("显示简介 → 共享与权限");
        expect(screen.getByRole("button", { name: "打开插件文件夹" })).toBeVisible();
        expect(screen.getByText(/\/theme\/plugin.json/)).toBeVisible();
    });

    it("should show a persistent error when scanning the plugin root is denied", async () => {
        mockInvokeHandler("list_plugins", () => Promise.reject("PLUGIN_FILE_PERMISSION_DENIED: /plugins: Permission denied"));
        render(<PluginCenterConfig pluginList={[]} />);
        expect(await screen.findByRole("alert")).toHaveTextContent("/plugins: Permission denied");
        expect(screen.getByRole("button", { name: "重新扫描插件" })).toBeVisible();
        expect(screen.queryByText("插件商店")).toBeNull();
    });
});
