import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { pluginRuntime } from "./PluginRuntime";

vi.mock("@tauri-apps/api/path", () => ({ appDataDir: async () => "/Application Support/Aipp" }));

describe("plugin script loading", () => {
    beforeEach(() => {
        mockInvokeHandler("get_enabled_plugins", () => [{
            pluginId: 1, code: "fixture", version: "1", name: "Fixture",
            isActive: true, permissions: [], pluginType: ["themeType"],
        }]);
    });
    afterEach(() => {
        vi.restoreAllMocks();
        document.getElementById("aipp-bootstrap")?.remove();
        Reflect.deleteProperty(window, "fixture");
    });

    it("should authorize the verified entry with the host page nonce", async () => {
        const bootstrap = document.createElement("script");
        bootstrap.id = "aipp-bootstrap";
        bootstrap.nonce = "per-page-value";
        document.head.appendChild(bootstrap);
        const verify = vi.fn();
        mockInvokeHandler("verify_plugin_entry_checksum", verify);
        vi.spyOn(document.body, "appendChild").mockImplementation((node) => {
            if (!(node instanceof HTMLScriptElement)) throw new Error("Expected a plugin script");
            expect(verify).toHaveBeenCalledWith({ code: "fixture", entry: "dist/main.js" });
            expect(node.nonce).toBe(bootstrap.nonce);
            Reflect.set(window, "fixture", class {});
            node.dispatchEvent(new Event("load"));
            return node;
        });
        const [result] = await pluginRuntime.reloadPlugins();
        expect(result.instance).toBeTruthy();
        expect(result.loadError).toBeUndefined();
    });

    it("should report a file permission error before injecting an unreadable entry", async () => {
        mockInvokeHandler("verify_plugin_entry_checksum", () => Promise.reject("PLUGIN_FILE_PERMISSION_DENIED: dist/main.js: Permission denied"));
        const append = vi.spyOn(document.body, "appendChild");
        vi.spyOn(console, "error").mockImplementation(() => {});
        const [result] = await pluginRuntime.reloadPlugins();
        expect(append).not.toHaveBeenCalled();
        expect(result.fileAccessDenied).toBe(true);
        expect(result.loadError).toContain("dist/main.js: Permission denied");
    });
});
