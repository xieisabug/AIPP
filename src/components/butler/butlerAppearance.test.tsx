import { afterEach, describe, expect, it } from "vitest";
import { avatarRegistry } from "@/components/avatar/avatarRegistry";
import { defaultAppearance, parseAppearance } from "./butlerAppearance";

describe("avatar appearance persistence", () => {
    afterEach(() => {
        avatarRegistry.clearPluginAssets(false);
    });

    it("should migrate existing colors when reading an unversioned appearance", () => {
        expect(parseAppearance(JSON.stringify({ shape: "aipp", left: "#CEF3DF", right: "#193B68" }))).toEqual({
            ...defaultAppearance, colors: { primary: "#CEF3DF", secondary: "#193B68" },
        });
    });
    it("should preserve the second shape and decorations when round-tripping", () => {
        const appearance = { ...defaultAppearance, shape: "pebble", accessories: { eyes: "round-glasses", head: "beret" }, motion: false };
        expect(parseAppearance(JSON.stringify(appearance))).toEqual(appearance);
        for (const accessories of [{ eyes: "sunglasses", head: "bow-bell" }, { eyes: "blush", head: "flower" }]) {
            const decorated = { ...defaultAppearance, accessories };
            expect(parseAppearance(JSON.stringify(decorated))).toEqual(decorated);
        }
    });
    it("should preserve namespaced plugin shapes and accessories across devices even when uninstalled", () => {
        const appearance = {
            ...defaultAppearance,
            shape: "cute-pets:orange-cat" as const,
            accessories: { eyes: "cute-pets:star-glasses" as const, head: "cute-pets:wizard-hat" as const },
        };
        expect(parseAppearance(JSON.stringify(appearance))).toEqual(appearance);
    });
    it("should validate slot mismatch for loaded plugin accessories", () => {
        avatarRegistry.setPluginAssets({
            avatars: [],
            accessories: [{
                id: "cute-pets:wizard-hat",
                localId: "wizard-hat",
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
        }, false);
        expect(parseAppearance(JSON.stringify({
            ...defaultAppearance,
            accessories: { eyes: "cute-pets:wizard-hat", head: null },
        }))).toBeUndefined();
        expect(parseAppearance(JSON.stringify({
            ...defaultAppearance,
            accessories: { eyes: null, head: "cute-pets:wizard-hat" },
        }))).toBeDefined();
    });
    it("should reject unsupported versions and malformed assets when reading configuration", () => {
        for (const value of [null, [], { ...defaultAppearance, version: 3 }, { ...defaultAppearance, shape: "__proto__" },
            { ...defaultAppearance, shape: "bad:id:extra" },
            { ...defaultAppearance, colors: { primary: "url(external)", secondary: "#000000" } },
            { ...defaultAppearance, accessories: { eyes: "unknown", head: null } },
            { ...defaultAppearance, accessories: { eyes: "beret", head: null } },
            { ...defaultAppearance, accessories: { eyes: null, head: "hasOwnProperty" } }]) {
            expect(parseAppearance(JSON.stringify(value))).toBeUndefined();
        }
    });
});
