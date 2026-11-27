import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import rawConsoleScript from "../../../src-tauri/artifacts/runtime/aipp-console.js?raw";
import {
    buildElementEditRequest,
    buildFixReport,
    compactScript,
    injectConsoleScript,
    previewInstanceId,
    readPickMessage,
    readRuntimeConsoleMessage,
    supportsElementPick,
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

const CONSOLE_METHODS = ["log", "info", "warn", "error", "debug"] as const;

/** 在当前文档里运行压成一行的诊断脚本，父窗口换成可观察的假对象。 */
function installRuntime() {
    const parent = { postMessage: vi.fn() };
    const fakeWindow = new Proxy(window, {
        get(target, key) {
            if (key === "parent") return parent;
            const value = Reflect.get(target, key);
            return typeof value === "function" ? value.bind(target) : value;
        },
        set(target, key, value) {
            return Reflect.set(target, key, value);
        },
    });
    new Function("window", compactScript(rawConsoleScript))(fakeWindow);
    const command = (value: "start" | "stop", source: unknown = parent) => {
        const event = new MessageEvent("message", { data: { __aippArtifactPick: value } });
        Object.defineProperty(event, "source", { value: source });
        window.dispatchEvent(event);
    };
    const picks = () =>
        parent.postMessage.mock.calls.map(([data]) => readPickMessage(data)).filter((message) => message !== null);
    return { command, picks };
}

describe("element picker in the runtime script", () => {
    const savedConsole = Object.fromEntries(CONSOLE_METHODS.map((name) => [name, console[name]]));

    beforeEach(() => {
        document.body.innerHTML =
            '<main><ul class="list"><li>一</li><li><button id="go" class="btn hover:bg-x">开始</button></li></ul><p class="note">说明</p></main>';
    });

    afterEach(() => {
        delete (window as unknown as Record<string, unknown>).__aippConsoleInstalled;
        delete (window as unknown as Record<string, unknown>).__aippDiag;
        for (const name of CONSOLE_METHODS) console[name] = savedConsole[name];
        document.documentElement.style.cursor = "";
        document.body.innerHTML = "";
    });

    it("should report the clicked element and keep the click from reaching the page", () => {
        const { command, picks } = installRuntime();
        const pageClick = vi.fn();
        document.getElementById("go")!.addEventListener("click", pageClick);

        command("start");
        expect(document.querySelector("[data-aipp-pick-overlay]")).toBeTruthy();
        document.getElementById("go")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));

        expect(pageClick).not.toHaveBeenCalled();
        expect(document.querySelector("[data-aipp-pick-overlay]")).toBeNull();
        expect(picks()).toEqual([
            {
                state: "picked",
                element: expect.objectContaining({
                    tag: "button",
                    selector: "button#go",
                    text: "开始",
                    html: expect.stringContaining('id="go"'),
                }),
            },
        ]);
    });

    it("should describe elements without an id by path, skipping utility classes", () => {
        const { command, picks } = installRuntime();
        command("start");
        document.querySelector("p.note")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        const [message] = picks();
        expect(message.state === "picked" && message.element.selector).toBe("main > p.note");

        command("start");
        document.querySelectorAll("li")[0].dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        const second = picks()[1];
        expect(second.state === "picked" && second.element.selector).toBe("main > ul.list > li:nth-of-type(1)");
    });

    it("should cancel with Escape and ignore commands from other windows", () => {
        const { command, picks } = installRuntime();
        command("start", {});
        expect(document.querySelector("[data-aipp-pick-overlay]")).toBeNull();

        command("start");
        document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        expect(document.querySelector("[data-aipp-pick-overlay]")).toBeNull();
        expect(picks()).toEqual([{ state: "cancelled" }]);
    });

    it("should stop when the host asks", () => {
        const { command, picks } = installRuntime();
        command("start");
        command("stop");
        document.getElementById("go")!.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true }));
        expect(picks()).toEqual([]);
        expect(document.documentElement.style.cursor).toBe("");
    });
});

describe("element edit helpers", () => {
    it("should only accept well-formed pick messages", () => {
        expect(readPickMessage({ __aippArtifactPick: 1, state: "cancelled" })).toEqual({ state: "cancelled" });
        expect(readPickMessage({ __aippArtifactPick: "start" })).toBeNull();
        expect(readPickMessage({ __aippArtifactPick: 1, state: "picked", element: { tag: 1 } })).toBeNull();
        expect(readPickMessage({ __aippArtifactConsole: 1 })).toBeNull();
    });

    it("should build a request naming the artifact, version and element", () => {
        const text = buildElementEditRequest({
            title: "番茄钟",
            type: "react",
            version: 3,
            instruction: " 改成圆角 ",
            element: { tag: "button", selector: "button#go", text: "开始", html: '<button id="go">开始</button>' },
        });
        expect(text).toContain("请修改 Artifact「番茄钟」（React） v3里的这个元素：改成圆角");
        expect(text).toContain("`button#go`");
        expect(text).toContain("```html\n<button id=\"go\">开始</button>\n```");
    });

    it("should offer picking only for pages that run the runtime script", () => {
        expect(["html", "react", "vue", "project"].every((type) => supportsElementPick(type as never))).toBe(true);
        expect(supportsElementPick("markdown")).toBe(false);
        expect(supportsElementPick(null)).toBe(false);
    });
});
