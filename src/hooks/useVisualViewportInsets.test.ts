import { afterEach, describe, expect, it, vi } from "vitest";
import { applyVisualViewportInsets, KEYBOARD_INSET_BOTTOM_VAR, VV_HEIGHT_VAR, VV_OFFSET_TOP_VAR } from "./useVisualViewportInsets";

describe("applyVisualViewportInsets", () => {
    afterEach(() => {
        document.documentElement.style.cssText = "";
        delete document.documentElement.dataset.aippKeyboardOpen;
        vi.unstubAllGlobals();
    });

    it("writes height/offset/keyboard inset from visualViewport", () => {
        const listeners: Record<string, Array<() => void>> = { resize: [], scroll: [] };
        const viewport = {
            height: 500,
            offsetTop: 40,
            addEventListener: (type: string, cb: () => void) => {
                listeners[type]?.push(cb);
            },
            removeEventListener: vi.fn(),
        };
        vi.stubGlobal("visualViewport", viewport);
        Object.defineProperty(window, "innerHeight", { configurable: true, value: 800 });

        const cleanup = applyVisualViewportInsets(document.documentElement);
        expect(document.documentElement.style.getPropertyValue(VV_HEIGHT_VAR)).toBe("500px");
        expect(document.documentElement.style.getPropertyValue(VV_OFFSET_TOP_VAR)).toBe("40px");
        expect(document.documentElement.style.getPropertyValue(KEYBOARD_INSET_BOTTOM_VAR)).toBe("260px");
        expect(document.documentElement.dataset.aippKeyboardOpen).toBe("true");

        cleanup();
        expect(document.documentElement.style.getPropertyValue(VV_HEIGHT_VAR)).toBe("");
    });
});
