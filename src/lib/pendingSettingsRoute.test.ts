import { describe, expect, it, beforeEach, afterEach, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { openSettings, takeSettingsRoute } from "./pendingSettingsRoute";

describe("cold settings navigation", () => {
    beforeEach(() => {
        const values = new Map<string, string>();
        vi.stubGlobal("localStorage", {
            setItem: (k: string, v: string) => values.set(k, v),
            getItem: (k: string) => values.get(k) ?? null,
            removeItem: (k: string) => values.delete(k),
        });
    });
    afterEach(() => {
        vi.unstubAllGlobals();
    });
    it("should deliver the requested sync page when settings mounts after the event", async () => {
        mockInvokeHandler("open_config_window", () => undefined);
        await openSettings("data-sync-config", undefined, "需要同步口令");
        const request = takeSettingsRoute();
        expect(request).toMatchObject({ menu: "data-sync-config", notice: "需要同步口令" });
        expect(takeSettingsRoute(request!)).toBeNull();
        expect(takeSettingsRoute()).toBeNull();
    });
    it("should preserve the latest pending destination if a superseded event arrives late", async () => {
        mockInvokeHandler("open_config_window", () => undefined);
        await openSettings("data-sync-config");
        await openSettings("network-config");
        expect(takeSettingsRoute({ menu: "data-sync-config", requestId: "old" })).toBeNull();
        expect(takeSettingsRoute()?.menu).toBe("network-config");
    });
    it("should continue to serve already open settings links", () => {
        expect(takeSettingsRoute({ menu: "assistant-config", subNav: "1" })).toEqual({
            menu: "assistant-config",
            subNav: "1",
        });
    });
});
