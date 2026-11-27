import { describe, expect, it } from "vitest";
import { clampPaneWidth } from "./ChatArtifactPane";

describe("clampPaneWidth", () => {
    it("should keep the pane between its minimum and the space left for the conversation", () => {
        expect(clampPaneWidth(200, 1400)).toBe(360);
        expect(clampPaneWidth(700, 1400)).toBe(700);
        expect(clampPaneWidth(1200, 1400)).toBe(980);
    });

    it("should keep the minimum width when the window is narrow", () => {
        expect(clampPaneWidth(600, 600)).toBe(360);
    });
});
