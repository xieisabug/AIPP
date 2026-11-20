import { invoke } from "@tauri-apps/api/core";

export interface OfficePreviewPluginStatus {
    pluginCode: string;
    installed: boolean;
    enabled: boolean;
    viewerReady: boolean;
    version?: string | null;
    localDevAvailable: boolean;
}

export interface PluginInstallProgressPayload {
    pluginCode: string;
    stage: "downloading" | "verifying" | "extracting" | "installing" | "done" | "error" | string;
    downloadedBytes: number;
    totalBytes?: number | null;
    percent?: number | null;
    message: string;
}

/** 自动安装失败后，这段时间内重新挂载的预览不再自动重试，避免离线时滚动历史反复触发下载。 */
export const OFFICE_PLUGIN_AUTO_RETRY_COOLDOWN_MS = 60_000;

let inflightInstall: Promise<OfficePreviewPluginStatus> | null = null;
let lastFailure: { at: number; message: string } | null = null;

export function isOfficePreviewPluginReady(status?: OfficePreviewPluginStatus | null): boolean {
    return Boolean(status?.installed && status.enabled && status.viewerReady);
}

export function getOfficePreviewPluginStatus(): Promise<OfficePreviewPluginStatus> {
    return invoke<OfficePreviewPluginStatus>("get_office_preview_plugin_status");
}

/**
 * 下载并安装 Office 预览插件。多个预览面板同时调用时共用同一次安装，
 * 进度通过 `plugin_install_progress` 事件广播给所有面板。
 */
export function installOfficePreviewPlugin(): Promise<OfficePreviewPluginStatus> {
    if (!inflightInstall) {
        inflightInstall = invoke<OfficePreviewPluginStatus>("install_office_preview_plugin", {})
            .then((status) => {
                lastFailure = null;
                return status;
            })
            .catch((err: unknown) => {
                const message = err instanceof Error ? err.message : String(err);
                lastFailure = { at: Date.now(), message };
                throw new Error(message);
            })
            .finally(() => {
                inflightInstall = null;
            });
    }
    return inflightInstall;
}

export function isOfficePreviewPluginInstalling(): boolean {
    return inflightInstall !== null;
}

/** 最近一次自动安装失败且仍在冷却期内时返回失败原因，否则返回 null。 */
export function recentOfficePluginInstallFailure(now = Date.now()): string | null {
    if (!lastFailure) return null;
    if (now - lastFailure.at > OFFICE_PLUGIN_AUTO_RETRY_COOLDOWN_MS) return null;
    return lastFailure.message;
}

export function enableOfficePreviewPlugin(): Promise<OfficePreviewPluginStatus> {
    return invoke<OfficePreviewPluginStatus>("enable_office_preview_plugin");
}

/** 仅供测试重置模块状态。 */
export function __resetOfficePreviewPluginStateForTests(): void {
    inflightInstall = null;
    lastFailure = null;
}
