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

        expect(host.shadowRoot?.textContent ?? "").toBe("");

        vi.advanceTimersByTime(6);
        expect(host.shadowRoot?.textContent).toContain("chunk-3");

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
});
