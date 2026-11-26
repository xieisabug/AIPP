import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { pluginRuntime, type LoadedPlugin } from "./PluginRuntime";

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
        Reflect.deleteProperty(window, "second-theme");
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

    it.each([
        [["hook.*"], "wildcards do not grant hooks"],
        [["*"], "wildcards do not grant hooks"],
        [["hook.chat.beforesend"], undefined],
    ])("should register hooks only with the exact hook permission (%j)", async (permissions, error) => {
        mockInvokeHandler("get_enabled_plugins", () => [{
            pluginId: 1, code: "fixture", version: "1", name: "Fixture",
            isActive: true, permissions, pluginType: ["applicationType"],
        }]);
        mockInvokeHandler("verify_plugin_entry_checksum", () => undefined);
        vi.spyOn(console, "error").mockImplementation(() => {});
        vi.spyOn(document.body, "appendChild").mockImplementation((node) => {
            Reflect.set(window, "fixture", class {
                onPluginLoad(api: { hooks: { register: (name: string, handler: () => void) => void } }) {
                    api.hooks.register("chat.beforeSend", () => {});
                }
            });
            node.dispatchEvent(new Event("load"));
            return node;
        });
        const [result] = await pluginRuntime.reloadPlugins();
        if (error) expect(result.loadError).toContain(error);
        else expect(result.loadError).toBeUndefined();
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

    it("should switch send button slots even when the enabled theme plugin always returns its leaf", async () => {
        const visual = "chat.input.send-button-visual";
        const icon = "chat.input.send-button-icon";
        const slots = [visual, icon].map((location) => ({ id: location, location, order: 10 }));
        mockInvokeHandler("get_enabled_plugins", () => ["fixture", "second-theme"].map((code, index) => ({
            pluginId: index + 1, code, version: "1", name: code,
            isActive: true, permissions: [], pluginType: ["themeType", "interfaceType"], contributions: { slots },
        })));
        mockInvokeHandler("verify_plugin_entry_checksum", () => undefined);
        const renderLeaf = vi.fn(() => "leaf");
        vi.spyOn(document.body, "appendChild").mockImplementation((node) => {
            if (!(node instanceof HTMLScriptElement)) throw new Error("Expected a plugin script");
            const isSecond = node.src.includes("/second-theme/");
            const themeId = isSecond ? "ocean" : "guofeng";
            Reflect.set(window, isSecond ? "second-theme" : "fixture", class {
                onPluginLoad(api: SystemApi) {
                    api.registerTheme({ id: themeId, label: themeId, variables: { primary: "red" } });
                }
                renderSlot = isSecond ? () => "wave" : renderLeaf;
            });
            node.dispatchEvent(new Event("load"));
            return node;
        });
        const plugins = await pluginRuntime.reloadPlugins();
        for (const location of [visual, icon] as const) {
            const render = (activeTheme: string) => pluginRuntime.renderSendButtonSlot(plugins, location, { activeTheme });
            expect(render("guofeng")).toBe("leaf");
            renderLeaf.mockClear();
            expect(render("default")).toBeNull();
            expect(render("newyear")).toBeNull();
            expect(render("ocean")).toBe("wave");
            expect(renderLeaf).not.toHaveBeenCalled();
            expect(render("guofeng")).toBe("leaf");
        }

        const iconPlugin: LoadedPlugin = {
            ...plugins[0], code: "custom-icon", pluginType: ["interfaceType"],
            contributions: { slots: [{ id: "custom", location: icon, order: 20 }] },
            instance: { config: () => ({ name: "Icon", type: ["interfaceType"] }), onPluginLoad() {}, renderSlot: () => "star" },
        };
        expect(pluginRuntime.renderSendButtonSlot([...plugins, iconPlugin], icon, { activeTheme: "default" })).toBe("star");
    });
});
