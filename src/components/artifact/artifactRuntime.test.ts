import { describe, expect, it } from "vitest";
import rawConsoleScript from "../../../src-tauri/artifacts/runtime/aipp-console.js?raw";
import {
    buildFixReport,
    compactScript,
    injectConsoleScript,
    previewInstanceId,
    readRuntimeConsoleMessage,
    toPreviewType,
    type ArtifactConsoleEntry,
} from "./artifactRuntime";

const entry = (level: ArtifactConsoleEntry["level"], message: string): ArtifactConsoleEntry => ({
    level,
    message,
    source: "runtime",
    time: 0,
});

describe("injectConsoleScript", () => {
    it("keeps page line numbers by injecting a single-line script that still parses", () => {
        const compacted = compactScript(rawConsoleScript);
        expect(compacted).not.toContain("\n");
        expect(() => new Function(compacted)).not.toThrow();
        const html = injectConsoleScript("<html><head>\n<script>app()</script></head></html>");
        expect(html.split("\n")).toHaveLength(2);
    });

    it("puts the diagnostics script right after <head>, before page scripts", () => {
        const html = injectConsoleScript("<html><header>x</header><head><script>app()</script></head></html>");
        const scriptAt = html.indexOf("__aippConsoleInstalled");
        expect(scriptAt).toBeGreaterThan(html.indexOf("<head>"));
        expect(scriptAt).toBeLessThan(html.indexOf("app()"));
    });

    it("falls back to <html> and then to the very beginning", () => {
        expect(injectConsoleScript("<html lang='zh'><body>x</body></html>").indexOf("<script>")).toBe(
            "<html lang='zh'>".length,
        );
        const fragment = injectConsoleScript("<svg></svg>");
        expect(fragment.startsWith("<script>")).toBe(true);
        expect(fragment.endsWith("<svg></svg>")).toBe(true);
    });
});

describe("readRuntimeConsoleMessage", () => {
    it("accepts only diagnostics messages", () => {
        expect(readRuntimeConsoleMessage({ id: "1", type: "db_query" })).toBeNull();
        expect(readRuntimeConsoleMessage("string")).toBeNull();
        expect(
            readRuntimeConsoleMessage({ __aippArtifactConsole: 1, entry: { level: "error", message: "boom", time: 5 } }),
        ).toEqual({ level: "error", message: "boom", source: "runtime", time: 5 });
        expect(
            readRuntimeConsoleMessage({ __aippArtifactConsole: 1, entry: { level: "weird", message: "x" } })?.level,
        ).toBe("log");
    });
});

describe("buildFixReport", () => {
    it("returns null when there are no errors", () => {
        expect(buildFixReport({ type: "html", entries: [entry("warn", "w"), entry("log", "l")] })).toBeNull();
    });

    it("lists each distinct error once with title and type", () => {
        const report = buildFixReport({
            title: "价格卡片",
            type: "react",
            entries: [entry("error", "TypeError: a"), entry("error", "TypeError: a"), entry("error", "ReferenceError: b")],
        })!;
        expect(report).toContain("「价格卡片」");
        expect(report).toContain("React");
        expect(report.match(/TypeError: a/g)).toHaveLength(1);
        expect(report).toContain("ReferenceError: b");
    });

    it("switches fence when an error contains backticks and caps length", () => {
        const fenced = buildFixReport({ type: "markdown", entries: [entry("error", "bad ``` block")] })!;
        expect(fenced).toContain("~~~~");
        const many = Array.from({ length: 40 }, (_, i) => entry("error", `${"x".repeat(400)} #${i}`));
        const report = buildFixReport({ type: "html", entries: many })!;
        expect(report.length).toBeLessThan(6500);
        expect(report).toMatch(/另有 \d+ 条错误未列出/);
    });
});

describe("preview helpers", () => {
    it("keeps the legacy instance id for the main preview window", () => {
        expect(previewInstanceId("react", "artifact_preview")).toBe("react");
        expect(previewInstanceId("vue", "sidebar")).toBe("vue-sidebar");
    });

    it("recognises preview types", () => {
        expect(toPreviewType("drawio")).toBe("drawio");
        expect(toPreviewType("python")).toBeNull();
        expect(toPreviewType(undefined)).toBeNull();
    });
});
