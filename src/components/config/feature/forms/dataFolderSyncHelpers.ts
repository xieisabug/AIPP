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
 * Decide whether「立即同步」should auto-save form values first.
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
            message: "请先填写服务器地址。填写完整后点「立即同步」会自动保存并同步",
        };
    }

    if (needsToken) {
        return {
            action: "need_token",
            message: "尚未配置访问 token。请先填写 token；填写后点「立即同步」会自动保存并同步",
        };
    }

    if (!input.tokenConfigured || formToken.length > 0 || urlDirty) {
        return { action: "save_then_sync" };
    }

    return { action: "sync_only" };
}

/** Map backend sync errors to friendlier Chinese copy when possible. */
export function friendlySyncError(raw: string, fallbackPrefix: string): string {
    const text = (raw || "").trim();
    if (!text) {
        return fallbackPrefix;
    }
    const lower = text.toLowerCase();
    if (lower.includes("aead") || lower.includes("decrypt secret") || text.includes("解密")) {
        return "本地安全 token 已失效（主密钥可能已更换）。请重新填写访问 token 并保存后再同步";
    }
    if (text.includes("缺少同步 token") || text.includes("缺少同步 Token")) {
        return "尚未保存访问 token。请填写 token 后点「立即同步」（会自动保存），或先点保存再同步";
    }
    if (text.includes("当前是本地模式")) {
        return "当前是本地模式，请先切换到「自建同步」并保存后再同步";
    }
    return `${fallbackPrefix}: ${text}`;
}
