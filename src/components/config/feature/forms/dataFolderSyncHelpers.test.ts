import { describe, expect, it } from "vitest";

import {
    canShowCleanupDangling,
    canShowRetryDeadLetters,
    canShowRetryFailed,
    canShowSyncNow,
    canStopSync,
    decideSyncNowAction,
    formatDanglingFkStatusText,
    formatRecentSyncError,
    formatSyncStatusText,
    friendlySyncError,
    isAutoSyncEnabled,
    normalizeSyncServerUrl,
    saveThenSyncSuccessMessage,
    shouldTriggerSyncAfterSave,
} from "./dataFolderSyncHelpers";

describe("dataFolderSyncHelpers", () => {
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

    it("formatSyncStatusText is compact connection + counters (no dangling / auto copy)", () => {
        expect(
            formatSyncStatusText({
                mode: "self_hosted",
                syncing: false,
                connected: true,
                running: true,
                paused: false,
                pending_outbox_count: 0,
                failed_outbox_count: 0,
                server_cursor: 3,
            }),
        ).toMatch(/^已连接；待推送/);
        expect(
            formatSyncStatusText({
                mode: "self_hosted",
                syncing: false,
                connected: false,
                running: false,
                paused: true,
                pending_outbox_count: 1,
                failed_outbox_count: 0,
                server_cursor: 3,
                dangling_fk_count: 5,
                dangling_fk_summary: "有 5 条历史引用已失效，已按空值同步",
            }),
        ).not.toContain("引用已失效");
        expect(
            formatSyncStatusText({
                mode: "self_hosted",
                syncing: false,
                connected: false,
                running: false,
                paused: true,
                pending_outbox_count: 1,
                failed_outbox_count: 0,
                server_cursor: 3,
            }),
        ).toContain("已连接");
    });

    it("canStopSync only for manual sync while auto-sync is off", () => {
        // Auto on + background/idle → no Stop
        expect(
            canStopSync({ mode: "self_hosted", syncing: false, running: true, paused: false }),
        ).toBe(false);
        // Auto on + syncing → still no Stop
        expect(
            canStopSync({ mode: "self_hosted", syncing: true, running: true, paused: false }),
        ).toBe(false);
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

    it("canShowSyncNow always in self_hosted; Stop is independent", () => {
        const autoSyncing = { mode: "self_hosted", syncing: false, running: true, paused: false };
        expect(canStopSync(autoSyncing)).toBe(false);
        expect(canShowSyncNow(autoSyncing)).toBe(true);

        const autoSyncRound = { mode: "self_hosted", syncing: true, running: true, paused: false };
        expect(canStopSync(autoSyncRound)).toBe(false);
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
            "有 2 条历史引用已失效，已按空值同步",
        );
        expect(formatDanglingFkStatusText({ dangling_fk_count: 0 })).toBeNull();
    });

    it("formatSyncStatusText does not append dangling FK notice", () => {
        expect(
            formatSyncStatusText({
                mode: "self_hosted",
                syncing: false,
                connected: true,
                running: true,
                pending_outbox_count: 0,
                failed_outbox_count: 0,
                server_cursor: 1,
                dangling_fk_count: 5,
                dangling_fk_summary: "有 5 条历史引用已失效，已按空值同步（conversation.message_attachment.message_id×5）",
            }),
        ).not.toContain("引用已失效");
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
