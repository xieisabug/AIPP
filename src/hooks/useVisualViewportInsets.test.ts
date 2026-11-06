import { afterEach, describe, expect, it, vi } from "vitest";
import { applyVisualViewportInsets, revealFocusedField, KEYBOARD_INSET_BOTTOM_VAR, VV_HEIGHT_VAR, VV_OFFSET_TOP_VAR } from "./useVisualViewportInsets";

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

    it("scrolls a dialog field that sits below its scroll body", () => {
        const dialog = document.createElement("div");
        dialog.setAttribute("data-slot", "dialog-content");
        const scroller = document.createElement("div");
        scroller.style.overflowY = "auto";
        const input = document.createElement("input");
        scroller.append(input);
        dialog.append(scroller);
        document.body.append(dialog);
        input.focus();

        const rect = (top: number, bottom: number): DOMRect => ({
            top, bottom, left: 0, right: 100, width: 100, height: bottom - top, x: 0, y: top, toJSON: () => ({}),
        });
        scroller.getBoundingClientRect = () => rect(100, 300);
        input.getBoundingClientRect = () => rect(360, 400);
        let scrolled = 0;
        Object.defineProperty(scroller, "scrollTop", {
            configurable: true,
            get: () => scrolled,
            set: (value: number) => { scrolled = value; },
        });

        revealFocusedField();
        expect(scrolled).toBe(108);
        dialog.remove();
    });
});
