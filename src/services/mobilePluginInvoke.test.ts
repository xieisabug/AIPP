import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { createPluginInvoke } from "./mobilePluginInvoke";
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("mobile plugin SDK bridge", () => {
    beforeEach(() => vi.clearAllMocks());
    it("should reject before dispatch when native capability validation fails", async () => {
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === "get_platform") return "android";
            throw new Error("MOBILE_UNSUPPORTED: plugin_id=7; command=execute_python");
        });
        await expect(createPluginInvoke(7)("execute_python")).rejects.toThrow("plugin_id=7");
        expect(invoke).not.toHaveBeenCalledWith("execute_python", expect.anything());
    });
    it("should bind plugin identity when an approved command is dispatched", async () => {
        vi.mocked(invoke).mockImplementation(async command => command === "get_platform" ? "android" : "ok");
        await createPluginInvoke(7)("get_plugin_data", { pluginId: 99, sessionId: "session" });
        expect(invoke).toHaveBeenLastCalledWith("get_plugin_data", { pluginId: 7, sessionId: "session" });
    });
    it("should fail without dispatch when platform lookup is unavailable", async () => {
        vi.mocked(invoke).mockRejectedValue(new Error("platform IPC unavailable"));
        await expect(createPluginInvoke(7)("get_plugin_data")).rejects.toThrow("platform IPC unavailable");
        expect(invoke).toHaveBeenCalledTimes(1);
    });
});
