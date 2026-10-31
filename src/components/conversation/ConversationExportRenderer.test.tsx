import { act } from "react";
import { describe, expect, it } from "vitest";
import { renderSingleMessageExportContent } from "./ConversationExportRenderer";

describe("export image URLs", () => {
    it("should preserve embedded raster images while rejecting script URLs", async () => {
        const container = document.createElement("div");
        const png = "data:image/png;base64,iVBORw0KGgo=";
        await act(async () => {
            renderSingleMessageExportContent(container, `![embedded](${png})\n\n[unsafe](javascript:alert%281%29)`, "user");
        });
        expect(container.querySelector("img")?.getAttribute("src")).toBe(png);
        expect(container.querySelector("a")?.getAttribute("href")).not.toContain("javascript:");
    });
});
