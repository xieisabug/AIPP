import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { pluginRuntime, type LoadedPlugin } from "@/services/PluginRuntime";
import PluginCenterConfig from "./PluginCenterConfig";

vi.mock("@/services/PluginRuntime", () => ({ pluginRuntime: { reloadPlugins: vi.fn(), listThemeVariablesOf: vi.fn(() => []) } }));

const plugin: LoadedPlugin = {
    pluginId: 1, name: "朱砂", code: "theme", version: "1", pluginType: ["themeType"], instance: null,
};
const officialSource = { folderName: "theme", kind: "official", officialId: "theme", sourceLabel: "x", installedAt: "2026-11-24T00:00:00Z" };
const installed = { ...plugin, isActive: true, isInstalled: true, installSource: officialSource };
const catalogEntry = {
    id: "dir", code: "dir", name: "目录速查", description: "!dir 快捷命令", version: "0.1.0", pluginTypes: ["toolType", "applicationType"],
    permissions: ["bang.register"], source: { type: "zip", url: "https://example.com/dir.zip" }, dirs: [{ from: ".", to: "dir" }], sha256: "abc",
};

const openMine = async () => {
    fireEvent.mouseDown(await screen.findByRole("tab", { name: /我的插件/ }));
    fireEvent.click(screen.getByRole("tab", { name: /我的插件/ }));
};
const openRow = async (name: string) => {
    await openMine();
    fireEvent.click(await screen.findByRole("button", { name: new RegExp(`^${name}`) }));
    return screen.findByRole("dialog");
};

describe("插件页", () => {
    beforeEach(() => {
        mockInvokeHandler("get_platform", () => "macos");
        mockInvokeHandler("list_plugins", () => [installed]);
        mockInvokeHandler("fetch_official_plugins", () => []);
        mockInvokeHandler("get_plugin_detail", () => null);
        vi.stubGlobal("innerWidth", 1440);
        for (const cmd of ["get_plugin_config", "get_plugin_hook_registrations", "list_plugin_hook_audit_logs"]) {
            mockInvokeHandler(cmd, () => []);
        }
        vi.mocked(pluginRuntime.reloadPlugins).mockReset();
        vi.mocked(invoke).mockClear();
    });

    it("should show a load failure in the detail sheet and clear it after retry", async () => {
        const reload = vi.mocked(pluginRuntime.reloadPlugins);
        reload.mockResolvedValueOnce([{ ...plugin, loadError: "WebView 未能加载插件脚本" }]);
        reload.mockResolvedValueOnce([{ ...plugin, instance: {} as AippPlugin }]);
        render(<PluginCenterConfig pluginList={[]} />);
        const sheet = await openRow("朱砂");
        expect(within(sheet).getByRole("alert")).toHaveTextContent("WebView 未能加载插件脚本");
        expect(within(sheet).queryByRole("button", { name: "打开插件文件夹" })).toBeNull();
        fireEvent.click(within(sheet).getByRole("button", { name: "重试加载" }));
        await waitFor(() => expect(within(sheet).queryByRole("alert")).toBeNull());
        expect(within(sheet).getByText("已启用")).toBeVisible();
    });

    it("should guide the user to file permissions when the OS denies directory access", async () => {
        mockInvokeHandler("list_plugins", () => [{ ...installed, entryAccessError: "/theme/plugin.json: Permission denied" }]);
        vi.mocked(pluginRuntime.reloadPlugins).mockResolvedValue([]);
        render(<PluginCenterConfig pluginList={[]} />);
        const sheet = await openRow("朱砂");
        expect(within(sheet).getByRole("alert")).toHaveTextContent("显示简介 → 共享与权限");
        expect(within(sheet).getByRole("button", { name: "打开插件文件夹" })).toBeVisible();
        expect(within(sheet).getByText(/\/theme\/plugin.json/)).toBeVisible();
    });

    it("should show a persistent error when scanning the plugin root is denied", async () => {
        mockInvokeHandler("list_plugins", () => Promise.reject("PLUGIN_FILE_PERMISSION_DENIED: /plugins: Permission denied"));
        render(<PluginCenterConfig pluginList={[]} />);
        expect(await screen.findByRole("alert")).toHaveTextContent("/plugins: Permission denied");
        expect(screen.getByRole("button", { name: "重新扫描插件" })).toBeVisible();
    });

    it("should preview declarative avatar and accessory plugins without requiring a JS view", async () => {
        mockInvokeHandler("list_plugins", () => [{
            pluginId: 2, name: "萌宠包", code: "cute-pets", version: "0.1.0", pluginType: ["avatarType"],
            runtime: { type: "declarative", entry: "plugin.json" },
            contributions: { avatars: ["avatars/cat.json"], accessories: ["accessories/hat.json"] },
            isActive: true, isInstalled: true,
        }]);
        mockInvokeHandler("get_plugin_avatar_assets", () => ({
            avatars: [{
                id: "cute-pets:cat", localId: "cat", pluginCode: "cute-pets", pluginName: "萌宠包", label: "小猫",
                vectorParts: [{ type: "circle", cx: 255, cy: 255, r: 150, fill: "secondary" }],
                face: { x: 255, y: 240, scale: 1 }, crown: { x: 255, y: 80, scale: 1 }, headphones: { x: 255, y: 80, scale: 1 },
                clip: { x: 140, y: 100, scale: 1 }, propDock: { x: 400, y: 350, scale: 0.8 }, colorLabels: { secondary: "身体" }, capabilities: ["eyes", "headwear"],
            }],
            accessories: [{
                id: "cute-pets:hat", localId: "hat", pluginCode: "cute-pets", pluginName: "萌宠包", label: "魔法帽", slot: "head", anchor: "crown",
                followGaze: false, action: "hat", previewViewBox: "-100 -100 200 200", layers: [{ type: "circle", cx: 0, cy: 0, r: 40, fill: "#3B3E82" }],
            }],
        }));
        vi.mocked(pluginRuntime.reloadPlugins).mockResolvedValue([]);
        render(<PluginCenterConfig pluginList={[]} />);
        const sheet = await openRow("萌宠包");
        expect(await within(sheet).findByText("包含的助手形象（1）")).toBeVisible();
        expect(within(sheet).getByText("小猫")).toBeVisible();
        expect(within(sheet).getByText("包含的配饰（1）")).toBeVisible();
        expect(within(sheet).getByText("魔法帽")).toBeVisible();
        expect(within(sheet).queryByRole("tab", { name: "设置" })).toBeNull();
    });

    it("should open as the market and install official plugins through the verified official path", async () => {
        mockInvokeHandler("fetch_official_plugins", () => [catalogEntry, { ...catalogEntry, id: "ui", code: "ui", name: "仪表盘", pluginTypes: ["interfaceType"] }]);
        mockInvokeHandler("install_plugin_archive_source", () => ({ installedPlugins: [] }));
        vi.mocked(pluginRuntime.reloadPlugins).mockResolvedValue([]);
        render(<PluginCenterConfig pluginList={[]} />);
        const card = await screen.findByRole("button", { name: /目录速查/ });
        expect(within(card).getByText("官方")).toBeVisible();
        expect(within(card).getByText("快捷命令")).toBeVisible();
        expect(within(card).getByText("未安装")).toBeVisible();
        const chips = screen.getByRole("toolbar", { name: "插件分类" });
        expect(chips).toHaveTextContent("快捷命令1");
        expect(chips).toHaveTextContent("界面1");
        fireEvent.click(card);
        const sheet = await screen.findByRole("dialog");
        expect(within(sheet).getByText("在输入框添加 ! 快捷命令")).toBeVisible();
        fireEvent.click(within(sheet).getByRole("button", { name: "安装" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("install_plugin_archive_source", expect.objectContaining({
            officialId: "dir", expectedSha256: "abc", selections: [{ from: ".", to: "dir" }],
        })));
    });

    it("should label third-party installs as unofficial", async () => {
        mockInvokeHandler("list_plugins", () => [{ ...installed, installSource: { ...officialSource, kind: "github", officialId: null, sourceLabel: "someone/theme#main" } }]);
        vi.mocked(pluginRuntime.reloadPlugins).mockResolvedValue([]);
        render(<PluginCenterConfig pluginList={[]} />);
        await openMine();
        const row = await screen.findByRole("button", { name: /^朱砂/ });
        expect(within(row).queryByText("官方")).toBeNull();
        expect(within(row).getByText(/来自 GitHub · someone\/theme#main/)).toBeVisible();
        fireEvent.click(row);
        const sheet = await screen.findByRole("dialog");
        expect(within(sheet).getByRole("note")).toHaveTextContent("非官方插件");
    });
});
