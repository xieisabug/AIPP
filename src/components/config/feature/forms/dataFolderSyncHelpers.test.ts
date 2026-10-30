import { describe, expect, it } from "vitest";

import {
    canReplaceSyncPassphrase,
    canShowCleanupDangling,
    canShowRetryDeadLetters,
    canShowRetryFailed,
    canShowSyncNow,
    canStopSync,
    decideSyncNowAction,
    duplicateProviderNotice,
    formatDanglingFkStatusText,
    formatLastSyncAt,
    formatRecentSyncError,
    formatSyncConnectionState,
    formatSyncStatusMetrics,
    friendlySyncError,
    isAutoSyncEnabled,
    normalizeSyncServerUrl,
    saveThenSyncSuccessMessage,
    secretSyncTone,
    shouldTriggerSyncAfterSave,
    showsSelfHostedSyncControls,
    syncConflictNotice,
    syncPassphraseActionLabel,
    validateSyncPassphrase,
} from "./dataFolderSyncHelpers";

describe("dataFolderSyncHelpers", () => {
    it("should ignore older polling counters when a newer conflict status was received", () => {
        const first = syncConflictNotice(undefined, 10, false);
        const next = syncConflictNotice(first.state, 12, false);
        expect(next.delta).toBe(2);
        const delayed = syncConflictNotice(next.state, 10, false);
        expect(delayed.delta).toBe(0);
        expect(syncConflictNotice(delayed.state, 12, false).delta).toBe(0);
    });
    it("normalizeSyncServerUrl strips trailing slashes", () => {
        expect(normalizeSyncServerUrl(" https://sync.example.com/ ")).toBe(
            "https://sync.example.com",
        );
    });

    it("asks for token when unset and form token empty", () => {
        const decision = decideSyncNowAction({
            formMode: "self_hosted",
            formServerUrl: "https://sync.example.com",
            formToken: "",
            tokenConfigured: false,
            savedServerUrl: "https://sync.example.com",
        });
        expect(decision.action).toBe("need_token");
    });

    it("auto-saves then syncs when form has token but not configured", () => {
        const decision = decideSyncNowAction({
            formMode: "self_hosted",
            formServerUrl: "https://sync.example.com",
            formToken: "new-token",
            tokenConfigured: false,
            savedServerUrl: "",
        });
        expect(decision).toEqual({ action: "save_then_sync" });
    });

    it("auto-saves when server url is dirty even if token already configured", () => {
        const decision = decideSyncNowAction({
            formMode: "self_hosted",
            formServerUrl: "https://new.example.com",
            formToken: "",
            tokenConfigured: true,
            savedServerUrl: "https://old.example.com",
        });
        expect(decision).toEqual({ action: "save_then_sync" });
    });

    it("syncs only when already configured and form not dirty", () => {
        const decision = decideSyncNowAction({
            formMode: "self_hosted",
            formServerUrl: "https://sync.example.com/",
            formToken: "",
            tokenConfigured: true,
            savedServerUrl: "https://sync.example.com",
        });
        expect(decision).toEqual({ action: "sync_only" });
    });

    it("maps decrypt/aead errors to friendly Chinese copy", () => {
        expect(friendlySyncError("Failed to decrypt secret: aead::Error", "触发同步失败")).toContain(
            "主密钥",
        );
        expect(friendlySyncError("缺少同步 token，请先保存自建同步配置", "触发同步失败")).toContain(
            "自动保存",
        );
        expect(friendlySyncError("同步已停止", "停止同步失败")).toBe("已停止同步");
        expect(
            friendlySyncError("变更过大被服务器拒绝（HTTP 413），已跳过并记入失败项", "触发同步失败"),
        ).toContain("HTTP 413");
    });

    it("formats connection state separately from queue metrics", () => {
        const connected = {
            mode: "self_hosted" as const,
            syncing: false,
            connected: true,
            running: true,
            paused: false,
            pending_outbox_count: 2,
            pushing_outbox_count: 1,
            failed_outbox_count: 3,
            dead_letter_count: 4,
            server_cursor: 9,
        };
        expect(formatSyncConnectionState(connected)).toBe("已连接");
        expect(formatSyncStatusMetrics(connected)).toEqual([
            { label: "待推送", value: 2 },
            { label: "推送中", value: 1 },
            { label: "失败", value: 3 },
            { label: "无法应用", value: 4 },
            { label: "游标", value: 9 },
        ]);
        expect(
            formatSyncStatusMetrics({
                ...connected,
                dangling_fk_count: 5,
                dangling_fk_summary: "有 5 条历史引用已失效，已按空值同步",
            }).map((metric) => metric.label),
        ).not.toContain("引用已失效");
        expect(
            formatSyncConnectionState({
                mode: "self_hosted",
                syncing: false,
                connected: false,
                running: false,
                paused: true,
                pending_outbox_count: 1,
                failed_outbox_count: 0,
                server_cursor: 3,
            }),
        ).toBe("已连接");
        expect(
            formatSyncConnectionState({
                mode: "local",
                syncing: false,
                connected: false,
                running: false,
                pending_outbox_count: 1,
                failed_outbox_count: 0,
                server_cursor: 3,
            }),
        ).toBe("本地模式");
        expect(
            formatSyncStatusMetrics({
                mode: "local",
                syncing: false,
                connected: false,
                running: false,
                pending_outbox_count: 1,
                failed_outbox_count: 0,
                server_cursor: 3,
            }),
        ).toEqual([]);
        expect(formatSyncConnectionState(null)).toBe("读取中");
    });

    it("formatLastSyncAt renders local time and falls back when empty or invalid", () => {
        expect(formatLastSyncAt(null)).toBe("尚未同步");
        expect(formatLastSyncAt("  ")).toBe("尚未同步");
        expect(formatLastSyncAt("not-a-date")).toBe("not-a-date");
        const iso = "2026-10-14T09:00:00+08:00";
        const date = new Date(iso);
        const pad = (n: number) => String(n).padStart(2, "0");
        expect(formatLastSyncAt(iso)).toBe(
            `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`,
        );
    });

    it("canStopSync when self_hosted and syncing (manual or auto)", () => {
        // Idle → no Stop
        expect(
            canStopSync({ mode: "self_hosted", syncing: false, running: true, paused: false }),
        ).toBe(false);
        // Auto on + syncing → Stop (cancel current round only)
        expect(
            canStopSync({ mode: "self_hosted", syncing: true, running: true, paused: false }),
        ).toBe(true);
        // Auto off + idle → no Stop
        expect(
            canStopSync({ mode: "self_hosted", syncing: false, running: false, paused: true }),
        ).toBe(false);
        // Auto off + manual sync in flight → Stop
        expect(
            canStopSync({ mode: "self_hosted", syncing: true, running: false, paused: true }),
        ).toBe(true);
        expect(canStopSync({ mode: "local", syncing: true, running: true, paused: true })).toBe(
            false,
        );
    });

    it("hides self-hosted sync status and controls in local mode", () => {
        expect(showsSelfHostedSyncControls("self_hosted")).toBe(true);
        expect(showsSelfHostedSyncControls("local")).toBe(false);
        expect(showsSelfHostedSyncControls(undefined)).toBe(false);
    });

    it("canShowSyncNow always in self_hosted; Stop when syncing", () => {
        const autoIdle = { mode: "self_hosted", syncing: false, running: true, paused: false };
        expect(canStopSync(autoIdle)).toBe(false);
        expect(canShowSyncNow(autoIdle)).toBe(true);

        const autoSyncRound = { mode: "self_hosted", syncing: true, running: true, paused: false };
        expect(canStopSync(autoSyncRound)).toBe(true);
        expect(canShowSyncNow(autoSyncRound)).toBe(true);

        const manualSyncing = { mode: "self_hosted", syncing: true, running: false, paused: true };
        expect(canStopSync(manualSyncing)).toBe(true);
        expect(canShowSyncNow(manualSyncing)).toBe(true);

        const paused = { mode: "self_hosted", syncing: false, running: false, paused: true };
        expect(canStopSync(paused)).toBe(false);
        expect(canShowSyncNow(paused)).toBe(true);

        expect(canShowSyncNow({ mode: "local", syncing: false, running: false })).toBe(false);
        expect(isAutoSyncEnabled({ mode: "self_hosted", paused: false })).toBe(true);
        expect(isAutoSyncEnabled({ mode: "self_hosted", paused: true })).toBe(false);
        expect(isAutoSyncEnabled({ mode: "local", paused: false })).toBe(false);
    });

    it("secondary retry/cleanup actions hide when counts are zero", () => {
        expect(canShowRetryFailed(0)).toBe(false);
        expect(canShowRetryFailed(2)).toBe(true);
        expect(canShowRetryDeadLetters(0)).toBe(false);
        expect(canShowRetryDeadLetters(1)).toBe(true);
        expect(canShowCleanupDangling(0)).toBe(false);
        expect(canShowCleanupDangling(3)).toBe(true);
    });

    it("shouldTriggerSyncAfterSave only when save skipped sync (needs_reset)", () => {
        expect(
            shouldTriggerSyncAfterSave({ mode: "self_hosted", needs_reset: false }),
        ).toBe(false);
        expect(
            shouldTriggerSyncAfterSave({ mode: "self_hosted", needs_reset: true }),
        ).toBe(true);
        expect(shouldTriggerSyncAfterSave({ mode: "local", needs_reset: true })).toBe(false);
    });

    it("saveThenSyncSuccessMessage merges to one toast", () => {
        expect(saveThenSyncSuccessMessage(false)).toContain("已开始同步");
        expect(saveThenSyncSuccessMessage(true)).toContain("开始同步");
    });

    it("formatDanglingFkStatusText prefers backend summary", () => {
        expect(
            formatDanglingFkStatusText({
                dangling_fk_count: 3,
                dangling_fk_summary: "有 3 条历史引用已失效，已按空值同步（conversation.message.conversation_id×3）",
            }),
        ).toContain("有 3 条历史引用已失效");
        expect(formatDanglingFkStatusText({ dangling_fk_count: 2 })).toBe(
            "有 2 条历史引用已失效，缺少必需归属的记录已暂停上传",
        );
        expect(formatDanglingFkStatusText({ dangling_fk_count: 0 })).toBeNull();
    });

    it("formatSyncStatusMetrics does not include dangling FK notice", () => {
        const metrics = formatSyncStatusMetrics({
            mode: "self_hosted",
            syncing: false,
            connected: true,
            running: true,
            pending_outbox_count: 0,
            failed_outbox_count: 0,
            server_cursor: 1,
            dangling_fk_count: 5,
            dangling_fk_summary: "有 5 条历史引用已失效，已按空值同步（conversation.message_attachment.message_id×5）",
        });
        expect(metrics.map((metric) => metric.label).join(" ")).not.toContain("引用已失效");
        expect(metrics.find((metric) => metric.label === "游标")?.value).toBe(1);
    });

    it("formatRecentSyncError softens stale outbox HTTP errors", () => {
        expect(
            formatRecentSyncError({
                last_error: "有 100 条待重试",
                failed_outbox_count: 100,
                syncing: false,
            }),
        ).toBe("有 100 条待重试");
        expect(
            formatRecentSyncError({
                last_error: "变更过大被服务器拒绝（HTTP 413），已跳过并记入失败项",
                failed_outbox_count: 100,
                syncing: false,
            }),
        ).toBe("有 100 条待重试");
        expect(
            formatRecentSyncError({
                last_error: null,
                failed_outbox_count: 5,
                syncing: false,
            }),
        ).toBe("有 5 条待重试");
        expect(
            formatRecentSyncError({
                last_error: "网络不可达",
                failed_outbox_count: 5,
                syncing: false,
            }),
        ).toBe("网络不可达");
        expect(
            formatRecentSyncError({
                last_error: null,
                failed_outbox_count: 0,
                syncing: false,
            }),
        ).toBe("无");
    });
});


describe("syncConflictNotice", () => {
    it("should ignore historical conflicts when settings first opens", () => {
        expect(syncConflictNotice(undefined, 100, false).delta).toBe(0);
    });
    it("should notify once when a round with several batches completes", () => {
        const baseline = syncConflictNotice(undefined, 100, false);
        const first = syncConflictNotice(baseline.state, 103, true);
        const second = syncConflictNotice(first.state, 108, true);
        expect(first.delta).toBe(0);
        expect(second.delta).toBe(0);
        const done = syncConflictNotice(second.state, 108, false);
        expect(done.delta).toBe(8);
        expect(syncConflictNotice(done.state, 108, false).delta).toBe(0);
    });
    it("should avoid a notice when an older counter arrives", () => {
        expect(syncConflictNotice({ count: 100, pending: 5 }, 0, false).delta).toBe(0);
    });
});

describe("encrypted API Key sync helpers", () => {
    it("maps backend states to tones and actions", () => {
        expect(secretSyncTone("ready")).toBe("ok");
        expect(secretSyncTone("needs_passphrase")).toBe("warn");
        expect(secretSyncTone("mismatch")).toBe("warn");
        expect(secretSyncTone("not_configured")).toBe("muted");
        expect(secretSyncTone(undefined)).toBe("muted");
        expect(syncPassphraseActionLabel("not_configured")).toBe("设置口令");
        expect(syncPassphraseActionLabel(undefined)).toBe("设置口令");
        expect(syncPassphraseActionLabel("needs_passphrase")).toBe("验证口令");
        expect(syncPassphraseActionLabel("mismatch")).toBe("验证口令");
        expect(syncPassphraseActionLabel("ready")).toBe("重新验证口令");
        expect(canReplaceSyncPassphrase("not_configured")).toBe(false);
        expect(canReplaceSyncPassphrase("mismatch")).toBe(true);
    });

    it("validates the passphrase length by characters", () => {
        expect(validateSyncPassphrase("")).toBe("请输入同步口令");
        expect(validateSyncPassphrase("1234567")).toContain("至少需要 8 个字符");
        expect(validateSyncPassphrase("八个汉字的同步口令")).toBeNull();
        expect(validateSyncPassphrase("12345678")).toBeNull();
    });

    it("only shows the duplicate provider notice when there is something to merge", () => {
        expect(duplicateProviderNotice(0)).toBeNull();
        expect(duplicateProviderNotice(undefined)).toBeNull();
        expect(duplicateProviderNotice(2)).toContain("发现 2 个同名重复的供应商");
    });
});
