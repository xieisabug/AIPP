import { useEffect } from "react";

export const VV_HEIGHT_VAR = "--aipp-vv-height";
export const VV_OFFSET_TOP_VAR = "--aipp-vv-offset-top";
export const KEYBOARD_INSET_BOTTOM_VAR = "--aipp-keyboard-inset-bottom";

/**
 * Publish visualViewport metrics as CSS variables for keyboard / IME layout.
 *
 * Evidence notes (LeaderAI #148):
 * - AndroidManifest uses windowSoftInputMode=adjustResize + MainActivity enableEdgeToEdge.
 * - On some API 35–37 WebViews, layout resize and visualViewport diverge: the top chrome can
 *   translate out while content paints under the system status bar, and the composer may sit
 *   under the IME. Owning layer is unclear (App chrome vs WebView/OS insets), so we:
 *   1) drive CSS from visualViewport (offsetTop / height / keyboard inset),
 *   2) keep safe-area padding,
 *   3) leave device verification to Coding Bot.
 *
 * Ownership (2026-11-03): MobileShell owns viewport geometry via height + translateY.
 * When the shell keyboard offset is active it forces --aipp-keyboard-inset-bottom: 0 on
 * the shell so chat composer does not double-lift. This document-level var remains for
 * sticky controls (e.g. ConfigForm) when the shell is not transforming.
 */
const FIELD_SELECTOR = "input, textarea, select, [contenteditable='true']";
const SHELL_SELECTOR = "[data-slot='dialog-content'], [data-aipp-slot='legacy-modal-panel'], [data-mobile-panel]";

/** Keep the focused field inside its dialog/modal scroller when the IME covers the layout viewport. */
export function revealFocusedField(): void {
    const active = document.activeElement;
    if (!(active instanceof HTMLElement) || !active.matches(FIELD_SELECTOR)) return;
    const shell = active.closest(SHELL_SELECTOR);
    if (!shell) return;

    let scroller: HTMLElement | null = active.parentElement;
    while (scroller && scroller !== shell) {
        const overflow = getComputedStyle(scroller).overflowY;
        if (overflow === "auto" || overflow === "scroll") break;
        scroller = scroller.parentElement;
    }
    if (!scroller || scroller === shell) return;

    const port = scroller.getBoundingClientRect();
    const rect = active.getBoundingClientRect();
    const pad = 8;
    if (rect.height >= port.height - pad * 2) {
        scroller.scrollTop += rect.top - port.top - pad;
        return;
    }
    if (rect.bottom > port.bottom - pad) {
        scroller.scrollTop += rect.bottom - port.bottom + pad;
    } else if (rect.top < port.top + pad) {
        scroller.scrollTop -= port.top + pad - rect.top;
    }
}

export function applyVisualViewportInsets(root: HTMLElement = document.documentElement): () => void {
    const viewport = window.visualViewport;
    if (!viewport) {
        root.style.setProperty(VV_HEIGHT_VAR, `${window.innerHeight}px`);
        root.style.setProperty(VV_OFFSET_TOP_VAR, "0px");
        root.style.setProperty(KEYBOARD_INSET_BOTTOM_VAR, "0px");
        return () => {
            root.style.removeProperty(VV_HEIGHT_VAR);
            root.style.removeProperty(VV_OFFSET_TOP_VAR);
            root.style.removeProperty(KEYBOARD_INSET_BOTTOM_VAR);
        };
    }

    const update = () => {
        const height = viewport.height;
        const offsetTop = viewport.offsetTop;
        // Prefer layout viewport (innerHeight) vs visual height for IME occlusion estimate.
        const keyboardInset = Math.max(0, window.innerHeight - height - offsetTop);
        root.style.setProperty(VV_HEIGHT_VAR, `${Math.round(height)}px`);
        root.style.setProperty(VV_OFFSET_TOP_VAR, `${Math.round(offsetTop)}px`);
        root.style.setProperty(KEYBOARD_INSET_BOTTOM_VAR, `${Math.round(keyboardInset)}px`);
        root.dataset.aippKeyboardOpen = keyboardInset > 80 ? "true" : "false";
        requestAnimationFrame(() => revealFocusedField());
    };

    const reveal = () => requestAnimationFrame(() => revealFocusedField());
    const onFocusIn = (event: FocusEvent) => {
        if (event.target instanceof Element && event.target.matches(FIELD_SELECTOR)) reveal();
    };
    update();
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    window.addEventListener("resize", update);
    document.addEventListener("focusin", onFocusIn);
    reveal();
    return () => {
        viewport.removeEventListener("resize", update);
        viewport.removeEventListener("scroll", update);
        window.removeEventListener("resize", update);
        document.removeEventListener("focusin", onFocusIn);
        root.style.removeProperty(VV_HEIGHT_VAR);
        root.style.removeProperty(VV_OFFSET_TOP_VAR);
        root.style.removeProperty(KEYBOARD_INSET_BOTTOM_VAR);
        delete root.dataset.aippKeyboardOpen;
    };
}

/** Subscribe for the app lifetime (mobile + narrow desktop forced-android screenshots). */
export function useVisualViewportInsets(enabled = true): void {
    useEffect(() => {
        if (!enabled || typeof window === "undefined") return;
        return applyVisualViewportInsets();
    }, [enabled]);
}
