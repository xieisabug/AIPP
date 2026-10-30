import { describe, expect, it } from "vitest";
import { withCurrentCodeThemeOption } from "./DisplayConfigForm";

const syntect = [
    { value: "InspiredGitHub", label: "InspiredGitHub" },
    { value: "Solarized (light)", label: "Solarized (light)" },
];

describe("withCurrentCodeThemeOption", () => {
    it("adds the legacy default id so the select is not blank", () => {
        const out = withCurrentCodeThemeOption(syntect, "github");
        expect(out[0]).toEqual({ value: "github", label: "GitHub" });
        expect(out).toHaveLength(3);
    });

    it("keeps options untouched when the value is already present or empty", () => {
        expect(withCurrentCodeThemeOption(syntect, "InspiredGitHub")).toBe(syntect);
        expect(withCurrentCodeThemeOption(syntect, "")).toBe(syntect);
    });

    it("uses the raw value as label for unknown ids", () => {
        expect(withCurrentCodeThemeOption(syntect, "my-theme")[0]).toEqual({ value: "my-theme", label: "my-theme" });
    });
});
