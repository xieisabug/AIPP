import { describe, expect, it } from "vitest";

import {
    decideSyncNowAction,
    friendlySyncError,
    normalizeSyncServerUrl,
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
    });
});
