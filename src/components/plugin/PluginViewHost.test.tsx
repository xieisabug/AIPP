import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import PluginViewHost from "./PluginViewHost";

describe("plugin view failure isolation", () => {
    it("should show the original load error when plugin initialization failed", () => {
        const plugin: any = { pluginId: 1, code: "fixture", name: "Fixture", instance: null, loadError: "plugin_id=1; stage=load: checksum mismatch", contributions: { views: [{ id: "view", location: "test", title: "Fixture" }] } };
        render(<PluginViewHost pluginList={[plugin]} location="test" />);
        expect(screen.getByRole("alert").textContent).toContain(plugin.loadError);
        expect(screen.queryByText("插件未实现 renderView()。")).toBeNull();
    });
    it("should preserve sibling content when a plugin React component throws", () => {
        const log = vi.spyOn(console, "error").mockImplementation(() => {});
        function Broken(): never { throw new Error("fixture render failed"); }
        const plugin: any = { pluginId: 1, code: "fixture", name: "Fixture", instance: { renderView: () => <Broken /> }, contributions: { views: [{ id: "view", location: "test", title: "Fixture" }] } };
        try {
            const { rerender } = render(<><p>聊天入口</p><PluginViewHost pluginList={[plugin]} location="test" /></>);
            expect(screen.getByRole("alert").textContent).toContain("fixture render failed");
            expect(screen.getByText("聊天入口")).toBeTruthy();
            rerender(<PluginViewHost pluginList={[{ ...plugin, instance: { renderView: () => <p>恢复后的插件</p> } }]} location="test" />);
            expect(screen.getByText("恢复后的插件")).toBeTruthy();
        } finally { log.mockRestore(); }
    });
});
