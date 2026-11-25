import { describe, expect, it } from "vitest";
import { parseSubNav, resolveConfigNavigation } from "./settingsNavigation";
import { searchSettings } from "./settingsSearchIndex";

describe("settingsNavigation", () => {
    it("resolves legacy feature-assistant-config subNavs to flat top-level pages", () => {
        expect(resolveConfigNavigation("feature-assistant-config", "display")).toEqual({
            menu: "general-config",
            subNav: undefined,
        });
        expect(resolveConfigNavigation("feature-assistant-config", "conversation_summary#auto-review-model")).toEqual({
            menu: "ai-automation-config",
            subNav: "#auto-review-model",
        });
        expect(resolveConfigNavigation("feature-assistant-config", "network_config")).toEqual({
            menu: "network-config",
            subNav: undefined,
        });
        expect(resolveConfigNavigation("feature-assistant-config", "preview")).toEqual({
            menu: "runtime-preview-config",
            subNav: undefined,
        });
        expect(resolveConfigNavigation("feature-assistant-config", "data_folder#passphrase")).toEqual({
            menu: "data-sync-config",
            subNav: "#passphrase",
        });
        expect(resolveConfigNavigation("feature-assistant-config", "experimental")).toEqual({
            menu: "butler-config",
            subNav: undefined,
        });
        expect(resolveConfigNavigation("feature-assistant-config", "shortcuts")).toEqual({
            menu: "shortcuts-config",
            subNav: undefined,
        });
        expect(resolveConfigNavigation("feature-assistant-config", "about")).toEqual({
            menu: "about-config",
            subNav: undefined,
        });
    });

    it("resolves canonical IDs and short aliases", () => {
        expect(resolveConfigNavigation("assistant-config", "7")).toEqual({
            menu: "assistant-config",
            subNav: "7",
        });
        expect(resolveConfigNavigation("butler", "feishu#butler-feishu")).toEqual({
            menu: "butler-config",
            subNav: "feishu#butler-feishu",
        });
        expect(resolveConfigNavigation("unknown-menu")).toBeNull();
    });

    it("parses tab#anchor subNav payloads", () => {
        expect(parseSubNav("policies#anti-leakage")).toEqual({
            tab: "policies",
            anchor: "anti-leakage",
            raw: "policies#anti-leakage",
        });
        expect(parseSubNav("#auto-review-model")).toEqual({
            tab: undefined,
            anchor: "auto-review-model",
            raw: "#auto-review-model",
        });
        expect(parseSubNav("feishu")).toEqual({
            tab: "feishu",
            anchor: undefined,
            raw: "feishu",
        });
    });
});

describe("settingsSearchIndex", () => {
    it("matches Chinese titles and English aliases", () => {
        const feishu = searchSettings("飞书", { isMobilePlatform: false });
        expect(feishu[0]?.menu).toBe("butler-config");
        expect(feishu[0]?.subNav).toBe("feishu#butler-feishu");

        const proxy = searchSettings("proxy", { isMobilePlatform: false });
        expect(proxy.some((item) => item.menu === "network-config")).toBe(true);

        const antiLeak = searchSettings("防泄露", { isMobilePlatform: false });
        expect(antiLeak[0]?.menu).toBe("vault-config");
        expect(antiLeak[0]?.subNav).toBe("policies#anti-leakage");
    });

    it("filters desktop-only settings on mobile platform", () => {
        const shortcutsMobile = searchSettings("快捷键", { isMobilePlatform: true });
        expect(shortcutsMobile.some((item) => item.menu === "shortcuts-config")).toBe(false);

        const bunMobile = searchSettings("bun", { isMobilePlatform: true });
        expect(bunMobile.some((item) => item.menu === "runtime-preview-config")).toBe(false);
    });
});
