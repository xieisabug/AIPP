export type MobileRoute = "chat_ui" | "config" | "collections" | "preview";
/** `collectionId` marks a preview opened from an existing collection item (no re-save offered). */
export interface MobilePreview { language: string; code: string; title?: string; collectionId?: number }
export function previewMobile(detail: MobilePreview): void {
    window.dispatchEvent(new CustomEvent("aipp:mobile-preview", { detail }));
}

export function navigateMobile(route: MobileRoute): void {
    window.dispatchEvent(new CustomEvent("aipp:mobile-navigate", { detail: route }));
}

/** Pop the current page without consulting page back handlers (e.g. after a leave prompt confirmed). */
export function popMobile(): void {
    window.dispatchEvent(new CustomEvent("aipp:mobile-pop"));
}

export function isMobileRoute(value: unknown): value is MobileRoute {
    return value === "chat_ui" || value === "config" || value === "collections" || value === "preview";
}

/** Returns true when the handler consumed the back press (the shell must not pop the page). */
export type MobileBackHandler = () => boolean;

const backHandlers = new Map<MobileRoute, MobileBackHandler[]>();

/** Let a page intercept system back before MobileShell pops it; the latest registration runs first. */
export function registerMobileBackHandler(route: MobileRoute, handler: MobileBackHandler): () => void {
    backHandlers.set(route, [...(backHandlers.get(route) ?? []), handler]);
    return () => {
        backHandlers.set(route, (backHandlers.get(route) ?? []).filter(item => item !== handler));
    };
}

export function runMobileBackHandlers(route: MobileRoute): boolean {
    return [...(backHandlers.get(route) ?? [])].reverse().some(handler => handler());
}
