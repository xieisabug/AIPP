/** Height drop that counts as the IME covering the page, not a browser chrome twitch. */
export const KEYBOARD_OPEN_THRESHOLD = 150;
/** Below this visible height the header and composer cannot keep their portrait sizes. */
export const KEYBOARD_COMPACT_HEIGHT = 220;
export const MOBILE_TEXTAREA_MIN_HEIGHT = 44;
export const MOBILE_TEXTAREA_MAX_VIEWPORT_RATIO = 0.3;

export function isKeyboardOpen(innerHeight: number, viewportHeight: number): boolean {
    return innerHeight - viewportHeight > KEYBOARD_OPEN_THRESHOLD;
}

/** Landscape IME: lock the composer to one line so it stays above the keyboard. */
export function isKeyboardCompact(innerHeight: number, viewportHeight: number): boolean {
    return isKeyboardOpen(innerHeight, viewportHeight) && viewportHeight < KEYBOARD_COMPACT_HEIGHT;
}

export function mobileTextareaMaxHeight(innerHeight: number, viewportHeight: number): number {
    if (isKeyboardCompact(innerHeight, viewportHeight)) return MOBILE_TEXTAREA_MIN_HEIGHT;
    return Math.max(MOBILE_TEXTAREA_MIN_HEIGHT, Math.round(viewportHeight * MOBILE_TEXTAREA_MAX_VIEWPORT_RATIO));
}
