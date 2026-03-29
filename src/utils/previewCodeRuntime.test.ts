import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPreviewCodeRuntime, type PreviewCodeBridge } from "@/utils/previewCodeRuntime";

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

    it("flushes streaming updates on animation frames instead of waiting for the stream to go idle", () => {
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
        vi.advanceTimersByTime(5);

        runtime.update({
            code: "<div>chunk-2</div>",
            isFinal: false,
            bridgeId: "preview-code-test",
            bridge,
        });
        vi.advanceTimersByTime(5);

        runtime.update({
            code: "<div>chunk-3</div>",
            isFinal: false,
            bridgeId: "preview-code-test",
            bridge,
        });

        expect(
            host.shadowRoot?.querySelector(".aipp-preview-code-root")?.textContent ?? ""
        ).toBe("");

        vi.advanceTimersByTime(6);
        expect(host.shadowRoot?.textContent).toContain("chunk-3");
        expect(
            host.shadowRoot?.querySelector(".aipp-preview-code-shell")?.getAttribute("data-streaming")
        ).toBe("true");

        runtime.update({
            code: "<div>chunk-4</div>",
            isFinal: false,
            bridgeId: "preview-code-test",
            bridge,
        });

        vi.advanceTimersByTime(16);
        expect(host.shadowRoot?.textContent).toContain("chunk-4");

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
