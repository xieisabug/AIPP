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
 */
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
    };

    update();
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    window.addEventListener("resize", update);
    return () => {
        viewport.removeEventListener("resize", update);
        viewport.removeEventListener("scroll", update);
        window.removeEventListener("resize", update);
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
