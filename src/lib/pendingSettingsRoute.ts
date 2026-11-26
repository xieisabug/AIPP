import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
export interface SettingsRouteRequest {
    menu: string;
    subNav?: string;
    notice?: string;
    requestId?: string;
}
const key = "aipp:pending-settings-route";
/** 启动新 WebView 时 JS 还未订阅事件。只保存导航目的地，设置页订阅后消费一次。 */
export async function openSettings(menu: string, subNav?: string, notice?: string) {
    const route: SettingsRouteRequest = { menu, subNav, notice, requestId: crypto.randomUUID() };
    localStorage.setItem(key, JSON.stringify(route));
    try {
        await invoke("open_config_window");
        await emit("config-navigate-to", route);
    } catch (error) {
        takeSettingsRoute(route);
        throw error;
    }
}
export function takeSettingsRoute(event?: SettingsRouteRequest): SettingsRouteRequest | null {
    if (event && !event.requestId) return event;
    const raw = localStorage.getItem?.(key);
    if (!raw) return null;
    try {
        const pending = JSON.parse(raw) as SettingsRouteRequest;
        if (event?.requestId && pending.requestId !== event.requestId) return null;
        localStorage.removeItem(key);
        return typeof pending.menu === "string" ? pending : null;
    } catch {
        localStorage.removeItem(key);
        return null;
    }
}
