import { describe, expect, it } from "vitest";

import {
    WINDOW_LABELS,
    actionIdToConfigKey,
    configKeyToActionId,
    getActionsByWindow,
} from "./Shortcuts";

describe("shortcut registry", () => {
    it("should register shared Butler shortcuts", () => {
        expect(getActionsByWindow("butler")).toEqual([
            expect.objectContaining({
                id: "butler.new",
                label: "重开新会话",
                defaultShortcut: "Mod+N",
            }),
            expect.objectContaining({
                id: "butler.stats",
                label: "查看消耗",
                defaultShortcut: "Mod+Shift+I",
            }),
            expect.objectContaining({
                id: "butler.settings",
                label: "打开设置",
                defaultShortcut: "Mod+Comma",
            }),
            expect.objectContaining({
                id: "butler.toggle_sidebar",
                label: "切换侧边栏",
                defaultShortcut: "Mod+B",
            }),
            expect.objectContaining({
                id: "butler.open_sidebar_window",
                label: "侧边详情窗口",
                defaultShortcut: "Mod+Shift+B",
            }),
        ]);
    });

    it("should expose Butler label and config key mapping", () => {
        expect(WINDOW_LABELS.butler).toBe("总管家窗口");
        expect(actionIdToConfigKey("butler.settings")).toBe("app.butler.settings");
        expect(configKeyToActionId("app.butler.toggle_sidebar")).toBe(
            "butler.toggle_sidebar"
        );
    });
});
