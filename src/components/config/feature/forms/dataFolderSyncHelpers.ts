/** Normalize sync server URL for dirty comparison (trim + strip trailing slashes). */
export function normalizeSyncServerUrl(url: string | null | undefined): string {
    return (url || "").trim().replace(/\/+$/, "");
}

export type SyncAutoSaveDecision =
    | { action: "sync_only" }
    | { action: "save_then_sync" }
    | { action: "need_token"; message: string }
    | { action: "need_server_url"; message: string }
    | { action: "wrong_mode"; message: string };

/**
 * Decide whether「立即同步一次」should auto-save form values first.
 * Prefer one-click: filled token / dirty URL → save then sync.
 */
export function decideSyncNowAction(input: {
    formMode: string;
    formServerUrl: string;
    formToken: string;
    tokenConfigured: boolean;
    savedServerUrl: string;
}): SyncAutoSaveDecision {
    const mode = input.formMode || "local";
    if (mode !== "self_hosted") {
        return {
            action: "wrong_mode",
            message: "当前是本地模式，请先切换到「自建同步」并保存后再同步",
        };
    }

    const formToken = (input.formToken || "").trim();
    const formUrl = normalizeSyncServerUrl(input.formServerUrl);
    const savedUrl = normalizeSyncServerUrl(input.savedServerUrl);
    const urlDirty = formUrl !== savedUrl;
    const needsToken = !input.tokenConfigured && !formToken;

    if (!formUrl) {
        return {
            action: "need_server_url",
            message: "请先填写服务器地址。填写完整后点「立即同步一次」会自动保存并同步",
        };
    }

    if (needsToken) {
        return {
            action: "need_token",
            message: "尚未配置访问 token。请先填写 token；填写后点「立即同步一次」会自动保存并同步",
        };
    }

    if (!input.tokenConfigured || formToken.length > 0 || urlDirty) {
        return { action: "save_then_sync" };
    }

    return { action: "sync_only" };
}

/**
 * After `save_sync_settings`, decide whether the UI still needs `trigger_sync_now`.
 * Normal self_hosted save (no needs_reset) already ran a sync in the backend —
 * calling sync-now again would double-sync. Only trigger when save skipped sync
 * (needs_reset) or the decision was sync-only (handled by the caller).
 */
export function shouldTriggerSyncAfterSave(saved: {
    mode: string;
    needs_reset?: boolean;
}): boolean {
    if (saved.mode !== "self_hosted") {
        return false;
    }
    // save_sync_settings skips run_sync_once when needs_reset is set.
    return Boolean(saved.needs_reset);
}

/** One success toast after save_then_sync (merged; no separate sync toast). */
export function saveThenSyncSuccessMessage(didTriggerSyncNow: boolean): string {
    if (didTriggerSyncNow) {
        return "配置已自动保存，开始同步…";
    }
    return "配置已自动保存并已开始同步";
}

export type SyncStatusSummaryInput = {
    mode: "local" | "self_hosted";
    syncing: boolean;
    connected: boolean;
    running: boolean;
    paused?: boolean;
    pending_outbox_count: number;
    pushing_outbox_count?: number;
    failed_outbox_count: number;
    dead_letter_count?: number;
    server_cursor: number;
    dangling_fk_count?: number;
    dangling_fk_summary?: string | null;
};

/** Friendly dangling-FK notice for status text / detail row. */
export function formatDanglingFkStatusText(
    status: Pick<SyncStatusSummaryInput, "dangling_fk_count" | "dangling_fk_summary"> | null | undefined,
): string | null {
    if (!status) {
        return null;
    }
    const summary = (status.dangling_fk_summary || "").trim();
    if (summary) {
        return summary;
    }
    const count = status.dangling_fk_count ?? 0;
    if (count > 0) {
        return `有 ${count} 条历史引用已失效，已按空值同步`;
    }
    return null;
}

/** Human-readable sync status line for the settings row. */
export function formatSyncStatusText(status: SyncStatusSummaryInput | null | undefined): string {
    if (!status) {
        return "读取中";
    }
    if (status.mode === "local") {
        return "本地模式：仅使用本机 SQLite，后台同步已停止。";
    }
    let state: string;
    if (status.paused && !status.syncing) {
        state = "后台同步已暂停";
    } else if (status.syncing) {
        state = "同步中";
    } else if (status.connected) {
        state = "已连接（后台自动同步中）";
    } else if (status.running) {
        state = "等待同步（后台自动同步已开启）";
    } else {
        state = "未运行";
    }
    let text = `${state}；待推送 ${status.pending_outbox_count}，推送中 ${status.pushing_outbox_count ?? 0}，失败 ${status.failed_outbox_count}，无法应用 ${status.dead_letter_count ?? 0}，游标 ${status.server_cursor}`;
    const dangling = formatDanglingFkStatusText(status);
    if (dangling) {
        text = `${text}。${dangling}`;
    }
    return text;
}

/** Whether「停止同步」should be enabled. */
export function canStopSync(input: {
    mode: string;
    syncing: boolean;
    running: boolean;
    paused?: boolean;
}): boolean {
    if (input.mode !== "self_hosted") {
        return false;
    }
    if (input.syncing) {
        return true;
    }
    if (input.running && !input.paused) {
        return true;
    }
    return false;
}

/** Map backend sync errors to friendlier Chinese copy when possible. */
export function friendlySyncError(raw: string, fallbackPrefix: string): string {
    const text = (raw || "").trim();
    if (!text) {
        return fallbackPrefix;
    }
    const lower = text.toLowerCase();
    if (text === "同步已停止" || text.includes("同步已停止")) {
        return "已停止同步";
    }
    if (text.includes("强制停止") || text.includes("协作取消超时")) {
        return "已强制停止同步（将在下次同步时自动恢复）";
    }
    if (text.includes("413") || text.includes("变更过大")) {
        return text.includes("已跳过")
            ? text
            : "某条变更过大被服务器拒绝，已跳过并记入失败项，其余同步继续";
    }
    if (lower.includes("aead") || lower.includes("decrypt secret") || text.includes("解密")) {
        return "本地安全 token 已失效（主密钥可能已更换）。请重新填写访问 token 并保存后再同步";
    }
    if (text.includes("缺少同步 token") || text.includes("缺少同步 Token")) {
        return "尚未保存访问 token。请填写 token 后点「立即同步一次」（会自动保存），或先点保存再同步";
    }
    if (text.includes("当前是本地模式")) {
        return "当前是本地模式，请先切换到「自建同步」并保存后再同步";
    }
    return `${fallbackPrefix}: ${text}`;
}
