import { describe, expect, it } from "vitest";
import { defaultAppearance, parseAppearance } from "./butlerAppearance";

describe("avatar appearance persistence", () => {
    it("should migrate existing colors when reading an unversioned appearance", () => {
        expect(parseAppearance(JSON.stringify({ shape: "aipp", left: "#CEF3DF", right: "#193B68" }))).toEqual({
            ...defaultAppearance, colors: { primary: "#CEF3DF", secondary: "#193B68" },
        });
    });
    it("should preserve the second shape and decorations when round-tripping", () => {
        const appearance = { ...defaultAppearance, shape: "pebble", accessories: { eyes: "round-glasses", head: "beret" }, motion: false };
        expect(parseAppearance(JSON.stringify(appearance))).toEqual(appearance);
    });
    it("should reject unsupported versions and malformed assets when reading configuration", () => {
        for (const value of [null, [], { ...defaultAppearance, version: 3 }, { ...defaultAppearance, shape: "__proto__" },
            { ...defaultAppearance, colors: { primary: "url(external)", secondary: "#000000" } },
            { ...defaultAppearance, accessories: { eyes: "unknown", head: null } }]) {
            expect(parseAppearance(JSON.stringify(value))).toBeUndefined();
        }
    });
});
