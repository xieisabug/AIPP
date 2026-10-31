import { describe, expect, it } from "vitest";
import { staticPreviewDocument } from "./previewDocument";

describe("staticPreviewDocument", () => {
    it("removes executable elements, event handlers and navigation links", () => {
        const html = staticPreviewDocument(`<script>alert(1)</script><iframe srcdoc="malicious"></iframe><object data="x"></object><form action="https://evil.test"><input></form><a href="javascript:alert(1)" target="_top" onclick="alert(1)">link</a><svg><a xlink:href="https://evil.test"><text>svg</text></a></svg><img src="https://evil.test/a" onerror="alert(1)">`);
        const doc = new DOMParser().parseFromString(html, "text/html");
        expect(doc.querySelector("script,iframe,object,form")).toBeNull();
        expect(doc.querySelector("a")?.hasAttribute("href")).toBe(false);
        expect(doc.querySelector("a")?.hasAttribute("target")).toBe(false);
        expect(doc.querySelector("[onclick], [onerror], [xlink\\:href]")).toBeNull();
        expect(doc.querySelector('meta[http-equiv="Content-Security-Policy"]')?.getAttribute("content")).toContain("img-src data:");
    });
});

