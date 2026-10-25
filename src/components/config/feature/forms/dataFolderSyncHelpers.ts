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

/** Human-readable sync status line for the connection-status row (no dangling notice). */
export function formatSyncStatusText(status: SyncStatusSummaryInput | null | undefined): string {
    if (!status) {
        return "读取中";
    }
    if (status.mode === "local") {
        return "本地模式：仅使用本机 SQLite，后台同步已停止。";
    }
    // Connection row: compact state + queue counters only (dangling lives in its own row).
    let state: string;
    if (status.syncing) {
        state = "同步中";
    } else if (status.connected || status.running || status.paused) {
        state = "已连接";
    } else {
        state = "未连接";
    }
    return `${state}；待推送 ${status.pending_outbox_count}，推送中 ${status.pushing_outbox_count ?? 0}，失败 ${status.failed_outbox_count}，无法应用 ${status.dead_letter_count ?? 0}，游标 ${status.server_cursor}`;
}

export type SyncPrimaryControlInput = {
    mode: string;
    syncing: boolean;
    running: boolean;
    paused?: boolean;
};

/**
 * Whether「停止同步」should show.
 * Only while a manual Sync Now round is in flight with auto-sync OFF (`paused`).
 * Auto-sync ON never shows Stop (turn auto off via the toggle instead).
 */
export function canStopSync(input: SyncPrimaryControlInput): boolean {
    if (input.mode !== "self_hosted") {
        return false;
    }
    return Boolean(input.syncing) && Boolean(input.paused);
}

/**
 * Whether「立即同步一次」should show.
 * Always available in self_hosted (not mutually exclusive with Stop the old way).
 * While a manual round is in flight (Stop visible), the button stays shown but disabled by the form.
 */
export function canShowSyncNow(input: SyncPrimaryControlInput): boolean {
    return input.mode === "self_hosted";
}

/** Auto-sync is on when not paused (paused mirrors persisted auto_sync=false). */
export function isAutoSyncEnabled(input: { mode?: string; paused?: boolean } | null | undefined): boolean {
    if (!input || input.mode !== "self_hosted") {
        return false;
    }
    return !Boolean(input.paused);
}

/** Secondary:「重试失败」— only when failed outbox items exist (not a third sync mode). */
export function canShowRetryFailed(failedOutboxCount: number): boolean {
    return (failedOutboxCount ?? 0) > 0;
}

/** Secondary:「重试无法应用的变更」— only when dead-letter count > 0. */
export function canShowRetryDeadLetters(deadLetterCount: number): boolean {
    return (deadLetterCount ?? 0) > 0;
}

/** Secondary:「清理失效引用」— only when dangling FK count > 0. */
export function canShowCleanupDangling(danglingFkCount: number): boolean {
    return (danglingFkCount ?? 0) > 0;
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

/**
 * 「最近错误」display: prefer a live runtime error; when idle with failed outbox
 * items only, show soft「有 N 条待重试」instead of raw stale HTTP 413 text.
 */
export function formatRecentSyncError(input: {
    last_error?: string | null;
    failed_outbox_count?: number;
    syncing?: boolean;
} | null | undefined): string {
    if (!input) {
        return "无";
    }
    const failed = input.failed_outbox_count ?? 0;
    const raw = (input.last_error || "").trim();
    if (!raw) {
        return !input.syncing && failed > 0 ? `有 ${failed} 条待重试` : "无";
    }
    if (/^有 \d+ 条待重试$/.test(raw)) {
        return raw;
    }
    // Defense: backend used to OR in ancient outbox last_error after a clean sync.
    if (
        !input.syncing &&
        failed > 0 &&
        (raw.includes("413") || raw.includes("变更过大") || /HTTP\s*\d+/i.test(raw))
    ) {
        return `有 ${failed} 条待重试`;
    }
    return raw;
}
