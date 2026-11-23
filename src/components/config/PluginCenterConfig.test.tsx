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

    it("should preview declarative avatar and accessory plugins without requiring a JS view", async () => {
        const avatarPlugin = {
            pluginId: 2,
            name: "萌宠包",
            code: "cute-pets",
            version: "0.1.0",
            pluginType: ["avatarType"],
            runtime: { type: "declarative", entry: "plugin.json" },
            contributions: { avatars: ["avatars/cat.json"], accessories: ["accessories/hat.json"] },
            isActive: true,
            isInstalled: true,
        };
        mockInvokeHandler("list_plugins", () => [avatarPlugin]);
        mockInvokeHandler("get_plugin_avatar_assets", () => ({
            avatars: [{
                id: "cute-pets:cat",
                localId: "cat",
                pluginCode: "cute-pets",
                pluginName: "萌宠包",
                label: "小猫",
                vectorParts: [{ type: "circle", cx: 255, cy: 255, r: 150, fill: "secondary" }],
                face: { x: 255, y: 240, scale: 1 },
                crown: { x: 255, y: 80, scale: 1 },
                headphones: { x: 255, y: 80, scale: 1 },
                clip: { x: 140, y: 100, scale: 1 },
                propDock: { x: 400, y: 350, scale: 0.8 },
                colorLabels: { secondary: "身体" },
                capabilities: ["eyes", "headwear"],
            }],
            accessories: [{
                id: "cute-pets:hat",
                localId: "hat",
                pluginCode: "cute-pets",
                pluginName: "萌宠包",
                label: "魔法帽",
                slot: "head",
                anchor: "crown",
                followGaze: false,
                action: "hat",
                previewViewBox: "-100 -100 200 200",
                layers: [{ type: "circle", cx: 0, cy: 0, r: 40, fill: "#3B3E82" }],
            }],
        }));
        vi.mocked(pluginRuntime.reloadPlugins).mockResolvedValue([]);
        render(<PluginCenterConfig pluginList={[]} />);
        expect(await screen.findByText("包含的助手形象（1）")).toBeVisible();
        expect(screen.getByText("小猫")).toBeVisible();
        expect(screen.getByText("包含的配饰（1）")).toBeVisible();
        expect(screen.getByText("魔法帽")).toBeVisible();
        expect(screen.queryByText(/不是 JS 运行时/)).toBeNull();
    });
});
