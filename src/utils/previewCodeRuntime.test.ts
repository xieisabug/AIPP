import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPreviewCodeRuntime, type PreviewCodeBridge } from "@/utils/previewCodeRuntime";
import { PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS } from "@/utils/previewCode";

const bridge: PreviewCodeBridge = {
    submit: vi.fn(async () => undefined),
    close: vi.fn(async () => undefined),
    emitEvent: vi.fn(),
};

describe("previewCodeRuntime", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
            return window.setTimeout(() => callback(performance.now()), 16);
        });
        vi.spyOn(window, "cancelAnimationFrame").mockImplementation((handle) => {
            window.clearTimeout(handle);
        });
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
        document.body.replaceChildren();
    });

    it("renders the first streaming chunk immediately and then throttles follow-up patches to 1s", () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);

        expect(host.shadowRoot?.querySelector(".aipp-preview-code-shell")).not.toBeNull();
        expect(host.shadowRoot?.querySelector("style")?.textContent).toContain(
            "@keyframes aipp-preview-code-enter"
        );

        runtime.update({
            code: "<div>chunk-1</div>",
            isFinal: false,
            bridgeId: "preview-code-test",
            bridge,
        });
        expect(
            host.shadowRoot?.querySelector(".aipp-preview-code-root")?.textContent ?? ""
        ).toBe("");
        vi.advanceTimersByTime(16);
        expect(host.shadowRoot?.textContent).toContain("chunk-1");
        expect(
            host.shadowRoot?.querySelector(".aipp-preview-code-shell")?.getAttribute("data-streaming")
        ).toBe("true");

        runtime.update({
            code: "<div>chunk-2</div>",
            isFinal: false,
            bridgeId: "preview-code-test",
            bridge,
        });
        vi.advanceTimersByTime(400);

        runtime.update({
            code: "<div>chunk-3</div>",
            isFinal: false,
            bridgeId: "preview-code-test",
            bridge,
        });

        vi.advanceTimersByTime(599);
        expect(host.shadowRoot?.textContent).toContain("chunk-1");

        vi.advanceTimersByTime(1);
        expect(host.shadowRoot?.textContent).toContain("chunk-3");

        runtime.destroy();
    });

    it("flushes the final update immediately even if a throttled streaming patch is pending", async () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);

        runtime.update({
            code: "<div id=\"status\">chunk-1</div>",
            isFinal: false,
            bridgeId: "preview-code-final-throttle-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        expect(host.shadowRoot?.textContent).toContain("chunk-1");

        runtime.update({
            code: "<div id=\"status\">chunk-2</div>",
            isFinal: false,
            bridgeId: "preview-code-final-throttle-test",
            bridge,
        });

        vi.advanceTimersByTime(100);
        runtime.update({
            code: "<div id=\"status\">final</div><script>document.getElementById(\"status\").setAttribute(\"data-ready\", \"true\");</script>",
            isFinal: true,
            bridgeId: "preview-code-final-throttle-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        await Promise.resolve();

        const status = host.shadowRoot?.querySelector<HTMLElement>("#status");
        expect(status?.textContent).toBe("final");
        expect(status?.getAttribute("data-ready")).toBe("true");

        runtime.destroy();
    });

    it("preserves scripts during streaming and executes them on final activation", async () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);

        runtime.update({
            code: '<div id="status">streaming</div><script>document.getElementById("status").textContent = "final";</script>',
            isFinal: false,
            bridgeId: "preview-code-script-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        expect(host.shadowRoot?.querySelector("script")).not.toBeNull();
        expect(host.shadowRoot?.textContent).toContain("streaming");

        runtime.update({
            code: '<div id="status">streaming</div><script>document.getElementById("status").textContent = "final";</script>',
            isFinal: true,
            bridgeId: "preview-code-script-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        await Promise.resolve();

        expect(host.shadowRoot?.textContent).toContain("final");

        runtime.destroy();
    });

    it("runs simple external classic scripts against the preview shadow document", async () => {
        vi.stubGlobal(
            "fetch",
            vi.fn(async () => ({
                ok: true,
                text: async () => `
                    const style = document.createElement("style");
                    const classNames = Array.from(document.querySelectorAll("[class]"))
                        .flatMap((element) => Array.from(element.classList));
                    style.textContent = classNames.map((className) => "." + className + "{display:flex;}").join("\\n");
                    document.head.append(style);
                `,
            }))
        );
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);

        runtime.update({
            code: '<div id="card" class="flex items-center"></div><script src="aipp-preview://localhost/tailwind-browser.js"></script>',
            isFinal: true,
            bridgeId: "preview-code-tailwind-browser-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        await vi.waitFor(() => {
            const shadowStyles = Array.from(host.shadowRoot?.querySelectorAll("style") ?? [])
                .map((style) => style.textContent ?? "")
                .join("\n");
            expect(shadowStyles).toContain(".flex{display:flex;}");
            expect(shadowStyles).toContain(".items-center{display:flex;}");
        });

        runtime.destroy();
    });

    it("exposes preview window globals to later inline scripts", async () => {
        vi.stubGlobal(
            "fetch",
            vi.fn(async () => ({
                ok: true,
                text: async () => `
                    (function(global) {
                        global.d3 = {
                            select(selector) {
                                return {
                                    text(value) {
                                        document.querySelector(selector).textContent = value;
                                    }
                                };
                            }
                        };
                    })(this);
                `,
            }))
        );
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const onError = vi.fn();

        runtime.update({
            code: '<div id="chart"></div><script src="aipp-preview://localhost/d3.js"></script><script>d3.select("#chart").text("loaded");</script>',
            isFinal: true,
            bridgeId: "preview-code-d3-global-test",
            bridge,
            onError,
        });

        vi.advanceTimersByTime(16);
        await vi.waitFor(() => {
            expect(host.shadowRoot?.querySelector("#chart")?.textContent).toBe("loaded");
        });
        expect(onError).toHaveBeenCalledWith(null);

        runtime.destroy();
    });

    it("allows inline scripts to read native window globals through the preview scope", async () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const onError = vi.fn();

        runtime.update({
            code: '<div id="clock"></div><script>document.getElementById("clock").textContent = String(typeof performance.now === "function");</script>',
            isFinal: true,
            bridgeId: "preview-code-performance-global-test",
            bridge,
            onError,
        });

        vi.advanceTimersByTime(16);
        await vi.waitFor(() => {
            expect(host.shadowRoot?.querySelector("#clock")?.textContent).toBe("true");
        });
        expect(onError).toHaveBeenCalledWith(null);

        runtime.destroy();
    });

    it("runs MutationObserver scripts through a shadow-root scoped observer", async () => {
        const observedTargets: Node[] = [];
        class FakeMutationObserver {
            constructor(private readonly callback: MutationCallback) {}

            observe(target: Node) {
                observedTargets.push(target);
                this.callback([{ target } as MutationRecord], this as unknown as MutationObserver);
            }

            disconnect() {}

            takeRecords() {
                return [];
            }
        }
        vi.stubGlobal("MutationObserver", FakeMutationObserver);
        vi.stubGlobal(
            "fetch",
            vi.fn(async () => ({
                ok: true,
                text: async () => `
                    const observer = new MutationObserver(() => {
                        const marker = document.createElement("span");
                        marker.id = "observer-ran";
                        document.body.appendChild(marker);
                    });
                    observer.observe(document.body, { childList: true, subtree: true });
                `,
            }))
        );
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const onError = vi.fn();

        runtime.update({
            code: '<div id="card" class="flex"></div><script src="aipp-preview://localhost/observer-script.js"></script>',
            isFinal: true,
            bridgeId: "preview-code-block-observer-script-test",
            bridge,
            onError,
        });

        vi.advanceTimersByTime(16);
        await vi.waitFor(() => {
            expect(observedTargets).toEqual([host.shadowRoot]);
        });
        vi.advanceTimersByTime(100);

        expect(host.shadowRoot?.querySelector("#observer-ran")).not.toBeNull();
        expect(onError).not.toHaveBeenCalledWith(expect.stringContaining("MutationObserver"));

        runtime.destroy();
    });

    it("falls back to real script loading for external module scripts", async () => {
        const appendChildSpy = vi.spyOn(document.head, "appendChild").mockImplementation((node) => {
            const script = node as HTMLScriptElement;
            queueMicrotask(() => {
                script.onload?.(new Event("load"));
            });
            return node;
        });
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);

        runtime.update({
            code: '<div>Module</div><script type="module" src="aipp-preview://localhost/module-entry.js"></script>',
            isFinal: true,
            bridgeId: "preview-code-module-script-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        await Promise.resolve();

        expect(appendChildSpy).toHaveBeenCalledTimes(1);
        const appendedScript = appendChildSpy.mock.calls[0]?.[0] as HTMLScriptElement;
        expect(appendedScript.tagName).toBe("SCRIPT");
        expect(appendedScript.type).toBe("module");
        expect(appendedScript.src).toBe("aipp-preview://localhost/module-entry.js");

        runtime.destroy();
    });

    it("retries fetching a classic external script after an earlier fetch failure", async () => {
        const fetchMock = vi
            .fn()
            .mockRejectedValueOnce(new Error("network down"))
            .mockResolvedValueOnce({
                ok: true,
                text: async () => `
                    const style = document.createElement("style");
                    style.textContent = ".recovered{display:block;}";
                    document.head.append(style);
                `,
            });
        vi.stubGlobal("fetch", fetchMock);

        const firstHost = document.createElement("div");
        document.body.appendChild(firstHost);
        const firstRuntime = createPreviewCodeRuntime(firstHost);
        const firstOnError = vi.fn();

        firstRuntime.update({
            code: '<div class="recovered"></div><script src="aipp-preview://localhost/retry-script.js"></script>',
            isFinal: true,
            bridgeId: "preview-code-script-retry-failure",
            bridge,
            onError: firstOnError,
        });

        vi.advanceTimersByTime(16);
        await vi.waitFor(() => {
            expect(firstOnError).toHaveBeenCalledWith("network down");
        });
        firstRuntime.destroy();

        const secondHost = document.createElement("div");
        document.body.appendChild(secondHost);
        const secondRuntime = createPreviewCodeRuntime(secondHost);
        const secondOnError = vi.fn();

        secondRuntime.update({
            code: '<div class="recovered"></div><script src="aipp-preview://localhost/retry-script.js"></script>',
            isFinal: true,
            bridgeId: "preview-code-script-retry-success",
            bridge,
            onError: secondOnError,
        });

        vi.advanceTimersByTime(16);
        await vi.waitFor(() => {
            const shadowStyles = Array.from(secondHost.shadowRoot?.querySelectorAll("style") ?? [])
                .map((style) => style.textContent ?? "")
                .join("\n");
            expect(shadowStyles).toContain(".recovered{display:block;}");
        });
        expect(fetchMock).toHaveBeenCalledTimes(2);
        expect(secondOnError).toHaveBeenCalledWith(null);

        secondRuntime.destroy();
    });

    it("keeps stable DOM nodes across streaming patches instead of replacing the whole subtree", () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);

        runtime.update({
            code: '<section id="card"><h2 id="title">Alpha</h2></section>',
            isFinal: false,
            bridgeId: "preview-code-stability-test",
            bridge,
        });

        vi.advanceTimersByTime(16);

        const initialCard = host.shadowRoot?.querySelector<HTMLElement>("#card");
        const initialTitle = host.shadowRoot?.querySelector<HTMLElement>("#title");
        expect(initialCard).not.toBeNull();
        expect(initialTitle?.textContent).toBe("Alpha");

        runtime.update({
            code: '<section id="card"><h2 id="title">Alpha</h2><p id="detail">Beta</p></section>',
            isFinal: false,
            bridgeId: "preview-code-stability-test",
            bridge,
        });

        vi.advanceTimersByTime(1000);

        const patchedCard = host.shadowRoot?.querySelector<HTMLElement>("#card");
        const patchedTitle = host.shadowRoot?.querySelector<HTMLElement>("#title");
        const detail = host.shadowRoot?.querySelector<HTMLElement>("#detail");

        expect(patchedCard).toBe(initialCard);
        expect(patchedTitle).toBe(initialTitle);
        expect(detail?.textContent).toBe("Beta");

        runtime.destroy();
    });

    it("keeps interactive listeners working when the bridge updates without code changes", async () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const firstBridge: PreviewCodeBridge = {
            submit: vi.fn(async () => undefined),
            close: vi.fn(async () => undefined),
            emitEvent: vi.fn(),
        };
        const secondBridge: PreviewCodeBridge = {
            submit: vi.fn(async () => undefined),
            close: vi.fn(async () => undefined),
            emitEvent: vi.fn(),
        };
        const code =
            '<button id="trigger">Run</button><script>document.getElementById("trigger").addEventListener("click", () => aippPreviewCode.submit({ status: "ok" }));</script>';

        runtime.update({
            code,
            isFinal: true,
            bridgeId: "preview-code-interaction-test",
            bridge: firstBridge,
        });

        vi.advanceTimersByTime(16);
        await Promise.resolve();

        const initialButton = host.shadowRoot?.querySelector<HTMLButtonElement>("#trigger");
        expect(initialButton).not.toBeNull();
        initialButton?.click();
        expect(firstBridge.submit).toHaveBeenCalledWith({ status: "ok" });

        runtime.update({
            code,
            isFinal: true,
            bridgeId: "preview-code-interaction-test",
            bridge: secondBridge,
        });

        vi.advanceTimersByTime(16);
        await Promise.resolve();

        const updatedButton = host.shadowRoot?.querySelector<HTMLButtonElement>("#trigger");
        expect(updatedButton).not.toBeNull();
        updatedButton?.click();
        expect(secondBridge.submit).toHaveBeenCalledWith({ status: "ok" });
        expect(host.shadowRoot?.textContent).toContain("Run");

        runtime.destroy();
    });

    it("binds inline onclick handlers to preview script globals", async () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const code = `
            <button id="trigger" onclick="startSimulation()">Run</button>
            <div id="status">idle</div>
            <script>
                function startSimulation() {
                    document.getElementById("status").textContent = "clicked";
                }
            </script>
        `;

        runtime.update({
            code,
            isFinal: true,
            bridgeId: "preview-code-inline-handler-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        await Promise.resolve();

        const button = host.shadowRoot?.querySelector<HTMLButtonElement>("#trigger");
        expect(button).not.toBeNull();
        button?.click();

        expect(host.shadowRoot?.textContent).toContain("clicked");

        runtime.destroy();
    });
});

// ---------------------------------------------------------------------------
// Streaming simulation: simulates a model progressively generating preview_code
// HTML and verifies that the runtime only patches the Shadow DOM once per second.
// ---------------------------------------------------------------------------

/**
 * Generates progressively larger HTML simulating a model building a page
 * token-by-token (compound interest calculator).
 */
function generateStreamingHtml(tokenIndex: number): string {
    const tokens = [
        '<div class="card">',
        "\n  ",
        "<h2>",
        "复利",
        "计算器",
        "</h2>",
        "\n  ",
        "<form>",
        "\n    ",
        "<label>",
        "本金：",
        "</label>",
        "\n    ",
        '<input type="number" id="principal" value="10000">',
        "\n    ",
        "<label>",
        "利率 (%)：",
        "</label>",
        "\n    ",
        '<input type="number" id="rate" value="5">',
        "\n    ",
        "<label>",
        "年数：",
        "</label>",
        "\n    ",
        '<input type="number" id="years" value="10">',
        "\n    ",
        '<button type="button" id="calculate">',
        "计算",
        "</button>",
        "\n  ",
        "</form>",
        "\n  ",
        '<div id="result">',
        "</div>",
        "\n",
        "</div>",
        "\n",
        "<style>",
        "\n  .card { font-family: sans-serif; padding: 16px; }",
        "\n  form { display: flex; flex-direction: column; gap: 8px; }",
        "\n  button { padding: 8px 16px; cursor: pointer; }",
        "\n</style>",
    ];
    return tokens.slice(0, Math.min(tokenIndex + 1, tokens.length)).join("");
}

describe("previewCodeRuntime streaming simulation", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => {
            return window.setTimeout(() => callback(performance.now()), 16);
        });
        vi.spyOn(window, "cancelAnimationFrame").mockImplementation((handle) => {
            window.clearTimeout(handle);
        });
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.useRealTimers();
        document.body.replaceChildren();
    });

    it("simulates 3 seconds of model streaming and verifies morphdom patches are ≤1/sec", () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const bridgeMock: PreviewCodeBridge = {
            submit: vi.fn(async () => undefined),
            close: vi.fn(async () => undefined),
            emitEvent: vi.fn(),
        };

        const CHUNK_INTERVAL_MS = 50;
        const TOTAL_CHUNKS = 60; // 60 * 50ms = 3000ms
        const THROTTLE_MS = PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS;

        // Record every actual DOM change via innerHTML (not textContent, to catch structural changes too)
        const patchTimeline: Array<{
            elapsedMs: number;
            chunkIndex: number;
            html: string;
        }> = [];

        let elapsedMs = 0;
        // Strip entrance-animation attributes so their cleanup timers don't create false DOM-change events
        const getMountHtml = () =>
            (host.shadowRoot?.querySelector(".aipp-preview-code-root")?.innerHTML ?? "")
                .replace(/ data-aipp-preview-enter="true"/g, "");
        let lastHtml = getMountHtml();

        const recordIfChanged = (chunkIndex: number) => {
            const html = getMountHtml();
            if (html !== lastHtml) {
                patchTimeline.push({ elapsedMs, chunkIndex, html });
                lastHtml = html;
            }
        };

        // --- Stream chunks ---
        for (let i = 0; i < TOTAL_CHUNKS; i++) {
            runtime.update({
                code: generateStreamingHtml(i),
                isFinal: false,
                bridgeId: "sim-stream",
                bridge: bridgeMock,
            });
            vi.advanceTimersByTime(CHUNK_INTERVAL_MS);
            elapsedMs += CHUNK_INTERVAL_MS;
            recordIfChanged(i);
        }

        // Let the last throttle window flush
        vi.advanceTimersByTime(THROTTLE_MS + 16);
        elapsedMs += THROTTLE_MS + 16;
        recordIfChanged(TOTAL_CHUNKS);

        // --- Send final update ---
        runtime.update({
            code: generateStreamingHtml(TOTAL_CHUNKS - 1),
            isFinal: true,
            bridgeId: "sim-stream",
            bridge: bridgeMock,
        });
        vi.advanceTimersByTime(16);
        elapsedMs += 16;
        recordIfChanged(TOTAL_CHUNKS + 1);

        // --- Print timeline for debugging ---
        // eslint-disable-next-line no-console
        console.log(
            "\n📊 Runtime patch timeline (morphdom calls):\n" +
            patchTimeline
                .map(
                    (p, idx) =>
                        `  [${idx}] t=${String(p.elapsedMs).padStart(5)}ms  chunk=${String(p.chunkIndex).padStart(3)}  html="${p.html.slice(0, 80)}${p.html.length > 80 ? "…" : ""}"`,
                )
                .join("\n"),
        );

        // --- Assertions ---
        // First patch should be the initial rAF (within first CHUNK_INTERVAL_MS + 16)
        expect(patchTimeline.length).toBeGreaterThanOrEqual(2);
        expect(patchTimeline[0].elapsedMs).toBeLessThanOrEqual(CHUNK_INTERVAL_MS + 16);

        // Subsequent streaming patches should be ≥1000ms apart
        for (let i = 1; i < patchTimeline.length - 1; i++) {
            const gap = patchTimeline[i].elapsedMs - patchTimeline[i - 1].elapsedMs;
            expect(
                gap,
                `Patch gap [${i - 1}]→[${i}]: ${gap}ms < ${THROTTLE_MS}ms`,
            ).toBeGreaterThanOrEqual(THROTTLE_MS - 50);
        }

        // For 3 seconds of streaming, we should have at most ~4 streaming patches + 1 final
        const maxExpectedPatches = Math.ceil((TOTAL_CHUNKS * CHUNK_INTERVAL_MS) / THROTTLE_MS) + 2;
        expect(patchTimeline.length).toBeLessThanOrEqual(maxExpectedPatches);

        runtime.destroy();
    });

    it("applies the very latest content when the throttle window expires", () => {
        const host = document.createElement("div");
        document.body.appendChild(host);
        const runtime = createPreviewCodeRuntime(host);
        const bridgeMock: PreviewCodeBridge = {
            submit: vi.fn(async () => undefined),
            close: vi.fn(async () => undefined),
            emitEvent: vi.fn(),
        };

        // First chunk – immediate
        runtime.update({
            code: "<p>v1</p>",
            isFinal: false,
            bridgeId: "latest-test",
            bridge: bridgeMock,
        });
        vi.advanceTimersByTime(16);
        expect(host.shadowRoot?.textContent).toContain("v1");

        // Rapid updates within throttle window
        for (let i = 2; i <= 20; i++) {
            runtime.update({
                code: `<p>v${i}</p>`,
                isFinal: false,
                bridgeId: "latest-test",
                bridge: bridgeMock,
            });
            vi.advanceTimersByTime(50);
        }

        // Should still show v1 (throttled)
        expect(host.shadowRoot?.textContent).toContain("v1");

        // Cross the throttle boundary
        vi.advanceTimersByTime(PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS);

        // Should show the LATEST version (v20), not an intermediate one
        expect(host.shadowRoot?.textContent).toContain("v20");

        runtime.destroy();
    });
});
