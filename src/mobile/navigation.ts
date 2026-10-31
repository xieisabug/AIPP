export type MobileRoute = "chat_ui" | "config" | "collections" | "preview";
export interface MobilePreview { language: string; code: string; title?: string }
export function previewMobile(detail: MobilePreview): void {
    window.dispatchEvent(new CustomEvent("aipp:mobile-preview", { detail }));
}

export function navigateMobile(route: MobileRoute): void {
    window.dispatchEvent(new CustomEvent("aipp:mobile-navigate", { detail: route }));
}

export function isMobileRoute(value: unknown): value is MobileRoute {
    return value === "chat_ui" || value === "config" || value === "collections" || value === "preview";
}
