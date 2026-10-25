import React, { useCallback, useEffect, useMemo, useState } from "react";
import { UseFormReturn, useWatch } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ConfigForm from "@/components/ConfigForm";
import ConfirmDialog from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";
import {
    canShowCleanupDangling,
    canShowRetryDeadLetters,
    canShowRetryFailed,
    canShowSyncNow,
    canStopSync,
    decideSyncNowAction,
    formatDanglingFkStatusText,
    formatSyncStatusText,
    friendlySyncError,
    saveThenSyncSuccessMessage,
    shouldTriggerSyncAfterSave,
} from "./dataFolderSyncHelpers";

interface DataFolderConfigFormProps {
    form: UseFormReturn<any>;
}

interface SyncStatus {
    mode: "local" | "self_hosted";
    server_url: string;
    token_configured: boolean;
    connected: boolean;
    running: boolean;
    syncing: boolean;
    paused?: boolean;
    last_sync_at?: string | null;
    last_error?: string | null;
    pending_outbox_count: number;
    pushing_outbox_count: number;
    failed_outbox_count: number;
    dead_letter_count: number;
    needs_reset: boolean;
    conflict_resolved_count: number;
    server_cursor: number;
    dangling_fk_count?: number;
    dangling_fk_summary?: string | null;
}

const STATUS_FALLBACK_POLL_MS = 30_000;

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({ form }) => {
    const [status, setStatus] = useState<SyncStatus | null>(null);
    const [saving, setSaving] = useState(false);
    const [syncing, setSyncing] = useState(false);
    const [stopping, setStopping] = useState(false);
    const [retryingFailed, setRetryingFailed] = useState(false);
    const [retryingDeadLetters, setRetryingDeadLetters] = useState(false);
    const [resetConfirmOpen, setResetConfirmOpen] = useState(false);
    const [resetting, setResetting] = useState(false);
    const [cleanupConfirmOpen, setCleanupConfirmOpen] = useState(false);
    const [cleaningDangling, setCleaningDangling] = useState(false);
    const watchedMode = useWatch({ control: form.control, name: "mode" });
    const mode = watchedMode || "local";
    const watchedServerUrl = useWatch({ control: form.control, name: "server_url" }) || "";
    const isPlaintextUrl = watchedServerUrl.trim().toLowerCase().startsWith("http://");

    const loadStatus = useCallback(async (syncFormValues = false) => {
        try {
            const nextStatus = await invoke<SyncStatus>("get_sync_status");
            setStatus(nextStatus);
            if (syncFormValues) {
                form.reset({
                    mode: nextStatus.mode || "local",
                    server_url: nextStatus.server_url || "",
                    token: "",
                });
            }
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "读取同步状态失败"));
        }
    }, [form]);

    useEffect(() => {
        void loadStatus(true);
        const unlistenPromise = listen<SyncStatus>("sync_status_changed", (event) => {
            setStatus((prev) => {
                const next = event.payload;
                const resolvedDelta = (next.conflict_resolved_count ?? 0) - (prev?.conflict_resolved_count ?? 0);
                if (resolvedDelta > 0) {
                    toast.warning(`有 ${resolvedDelta} 处修改与服务器冲突，已采用服务器版本`);
                }
                return next;
            });
        });
        return () => {
            void unlistenPromise.then((unlisten) => unlisten());
        };
    }, [loadStatus]);

    useEffect(() => {
        if (mode !== "self_hosted") {
            return;
        }
        const timer = window.setInterval(() => {
            void loadStatus(false);
        }, STATUS_FALLBACK_POLL_MS);
        return () => window.clearInterval(timer);
    }, [mode, loadStatus]);

    useEffect(() => {
        const refresh = () => {
            void loadStatus(false);
        };
        const refreshWhenVisible = () => {
            if (document.visibilityState === "visible") {
                refresh();
            }
        };
        window.addEventListener("focus", refresh);
        document.addEventListener("visibilitychange", refreshWhenVisible);
        return () => {
            window.removeEventListener("focus", refresh);
            document.removeEventListener("visibilitychange", refreshWhenVisible);
        };
    }, [loadStatus]);

    const handleOpenDataFolder = useCallback(async () => {
        try {
            await invoke("open_data_folder");
        } catch (error) {
            toast.error("打开数据目录失败: " + getErrorMessage(error));
        }
    }, []);

    const persistSyncSettings = useCallback(async (opts?: { quietSuccess?: boolean }) => {
        const values = form.getValues();
        const nextStatus = await invoke<SyncStatus>("save_sync_settings", {
            request: {
                mode: values.mode || "local",
                server_url: values.server_url || "",
                token: values.token || "",
            },
        });
        setStatus(nextStatus);
        form.reset({
            mode: nextStatus.mode || "local",
            server_url: nextStatus.server_url || "",
            token: "",
        });
        if (!opts?.quietSuccess) {
            toast.success(
                nextStatus.mode === "self_hosted"
                    ? "自建同步配置已保存，后台将自动同步"
                    : "已切换为本地模式",
            );
        }
        if (nextStatus.mode === "self_hosted") {
            void loadStatus(false);
        }
        return nextStatus;
    }, [form, loadStatus]);

    const handleSave = useCallback(async () => {
        setSaving(true);
        try {
            await persistSyncSettings();
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "保存同步配置失败"));
        } finally {
            setSaving(false);
        }
    }, [persistSyncSettings]);

    const handleSyncNow = useCallback(async () => {
        const values = form.getValues();
        const decision = decideSyncNowAction({
            formMode: values.mode || "local",
            formServerUrl: values.server_url || "",
            formToken: values.token || "",
            tokenConfigured: Boolean(status?.token_configured),
            savedServerUrl: status?.server_url || "",
        });

        if (decision.action === "wrong_mode" || decision.action === "need_token" || decision.action === "need_server_url") {
            toast.error(decision.message);
            return;
        }

        setSyncing(true);
        try {
            if (decision.action === "save_then_sync") {
                setSaving(true);
                let saved: SyncStatus;
                try {
                    saved = await persistSyncSettings({ quietSuccess: true });
                    if (saved.mode === "self_hosted" && !saved.token_configured) {
                        toast.error("访问 token 未能保存成功，请检查后重试");
                        return;
                    }
                } finally {
                    setSaving(false);
                }

                // Normal self_hosted save already ran sync; only trigger again
                // when save skipped sync (needs_reset) — one merged success toast.
                const needSyncNow = shouldTriggerSyncAfterSave(saved);
                if (needSyncNow) {
                    const nextStatus = await invoke<SyncStatus>("trigger_sync_now");
                    setStatus(nextStatus);
                } else {
                    setStatus(saved);
                }
                toast.success(saveThenSyncSuccessMessage(needSyncNow));
                return;
            }

            // sync_only: save did not run, so trigger sync now.
            const nextStatus = await invoke<SyncStatus>("trigger_sync_now");
            setStatus(nextStatus);
            toast.success("已触发一次同步（平时会自动同步）");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "触发同步失败"));
        } finally {
            setSyncing(false);
        }
    }, [form, persistSyncSettings, status?.server_url, status?.token_configured]);

    const handleStopSync = useCallback(async () => {
        setStopping(true);
        try {
            const nextStatus = await invoke<SyncStatus>("stop_sync");
            setStatus(nextStatus);
            toast.success(nextStatus.paused ? "已停止同步，后台同步已暂停" : "已停止同步");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "停止同步失败"));
        } finally {
            setStopping(false);
        }
    }, []);

    const handleRetryFailed = useCallback(async () => {
        setRetryingFailed(true);
        try {
            const nextStatus = await invoke<SyncStatus>("retry_failed_sync_outbox");
            setStatus(nextStatus);
            toast.success("已重新触发失败项");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "重试失败项失败"));
        } finally {
            setRetryingFailed(false);
        }
    }, []);

    const handleRetryDeadLetters = useCallback(async () => {
        setRetryingDeadLetters(true);
        try {
            const nextStatus = await invoke<SyncStatus>("retry_sync_dead_letters");
            setStatus(nextStatus);
            toast.success("已重试无法应用的变更");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "重试无法应用的变更失败"));
        } finally {
            setRetryingDeadLetters(false);
        }
    }, []);

    const handleResetSyncState = useCallback(async () => {
        setResetConfirmOpen(false);
        setResetting(true);
        try {
            const nextStatus = await invoke<SyncStatus>("reset_sync_state");
            setStatus(nextStatus);
            toast.success("同步状态已重置，正在重新全量同步");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "重置同步状态失败"));
        } finally {
            setResetting(false);
        }
    }, []);

    const handleCleanupDanglingFks = useCallback(async () => {
        setCleanupConfirmOpen(false);
        setCleaningDangling(true);
        try {
            const nextStatus = await invoke<SyncStatus>("cleanup_dangling_foreign_keys");
            setStatus(nextStatus);
            const notice = formatDanglingFkStatusText(nextStatus);
            toast.success(notice || "已清理失效的本地引用");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "清理失效引用失败"));
        } finally {
            setCleaningDangling(false);
        }
    }, []);

    const statusText = useMemo(() => formatSyncStatusText(status), [status]);
    const danglingText = useMemo(() => formatDanglingFkStatusText(status), [status]);
    const danglingCount = status?.dangling_fk_count ?? 0;
    const primaryControlInput = {
        mode,
        syncing: Boolean(status?.syncing) || syncing,
        running: Boolean(status?.running),
        paused: status?.paused,
    };
    const stopEnabled = canStopSync(primaryControlInput);
    const showSyncNow = canShowSyncNow(primaryControlInput);
    const failedOutboxCount = status?.failed_outbox_count ?? 0;
    const deadLetterCount = status?.dead_letter_count ?? 0;
    const showRetryFailed = canShowRetryFailed(failedOutboxCount);
    const showRetryDeadLetters = canShowRetryDeadLetters(deadLetterCount);
    const showCleanupDangling = canShowCleanupDangling(danglingCount);

    const renderSyncStatusRow = useCallback(() => {
        const items = [
            { label: "连接状态", value: statusText },
            { label: "上次同步时间", value: status?.last_sync_at || "尚未同步" },
            { label: "最近错误", value: status?.last_error || "无" },
        ];
        return (
            <div className="space-y-3">
                <div className="grid grid-cols-1 gap-3 md:grid-cols-3">
                    {items.map((item) => (
                        <div key={item.label} className="min-w-0 space-y-2">
                            <div className="font-semibold text-sm text-foreground">{item.label}</div>
                            <div className="text-sm text-muted-foreground px-3 py-2 bg-muted rounded-md break-words whitespace-pre-wrap">
                                {item.value}
                            </div>
                        </div>
                    ))}
                </div>
                {danglingText ? (
                    <div className="text-sm text-amber-700 dark:text-amber-300 px-3 py-2 bg-amber-50 dark:bg-amber-950/40 rounded-md break-words">
                        {danglingText}
                        {danglingCount > 0
                            ? " 如需把本机对应字段也清空，可点下方「清理失效引用」。"
                            : ""}
                    </div>
                ) : null}
            </div>
        );
    }, [danglingCount, danglingText, status?.last_error, status?.last_sync_at, statusText]);

    const renderSyncActionButtons = useCallback(() => {
        // Primary: Sync Now | Stop are mutually exclusive. Secondary retries only when counts > 0.
        return (
            <div className="flex flex-wrap items-center gap-2">
                {showSyncNow ? (
                    <Button
                        type="button"
                        variant="outline"
                        className="hover:bg-muted hover:border-border"
                        disabled={syncing || saving || stopping}
                        onClick={handleSyncNow}
                    >
                        {syncing
                            ? "同步中..."
                            : status?.token_configured
                              ? "立即同步一次"
                              : "保存并同步"}
                    </Button>
                ) : null}
                {stopEnabled ? (
                    <Button
                        type="button"
                        variant="outline"
                        className="hover:bg-muted hover:border-border"
                        disabled={stopping || saving}
                        onClick={handleStopSync}
                    >
                        {stopping ? "停止中..." : "停止同步"}
                    </Button>
                ) : null}
                {showRetryFailed ? (
                    <Button
                        type="button"
                        variant="outline"
                        className="hover:bg-muted hover:border-border"
                        disabled={retryingFailed || saving}
                        onClick={handleRetryFailed}
                    >
                        {retryingFailed ? "重试中..." : "重试失败"}
                    </Button>
                ) : null}
                {showRetryDeadLetters ? (
                    <Button
                        type="button"
                        variant="outline"
                        className="hover:bg-muted hover:border-border"
                        disabled={retryingDeadLetters || saving}
                        onClick={handleRetryDeadLetters}
                    >
                        {retryingDeadLetters ? "重试中..." : "重试无法应用的变更"}
                    </Button>
                ) : null}
                {showCleanupDangling ? (
                    <Button
                        type="button"
                        variant="outline"
                        className="hover:bg-muted hover:border-border"
                        disabled={cleaningDangling || saving}
                        onClick={() => setCleanupConfirmOpen(true)}
                    >
                        {cleaningDangling ? "清理中..." : "清理失效引用"}
                    </Button>
                ) : null}
            </div>
        );
    }, [
        cleaningDangling,
        handleRetryDeadLetters,
        handleRetryFailed,
        handleStopSync,
        handleSyncNow,
        retryingDeadLetters,
        retryingFailed,
        saving,
        showCleanupDangling,
        showRetryDeadLetters,
        showRetryFailed,
        showSyncNow,
        status?.token_configured,
        stopEnabled,
        stopping,
        syncing,
    ]);

    const dataFolderConfig = [
        {
            key: "openDataFolder",
            config: {
                type: "button" as const,
                label: "数据文件夹",
                value: "打开",
                onClick: handleOpenDataFolder,
            },
        },
        {
            key: "mode",
            config: {
                type: "radio" as const,
                label: "同步模式",
                options: [
                    {
                        value: "local",
                        label: "本地模式",
                        tooltip: "只使用本机 SQLite，并停止后台同步。",
                    },
                    {
                        value: "self_hosted",
                        label: "自建同步",
                        tooltip: "连接你自建的 AIPP sync-server。配置完成后本地改动与远端变更会自动同步，一般无需点「立即同步一次」。",
                    },
                ],
            },
        },
        {
            key: "server_url",
            config: {
                type: "input" as const,
                label: "服务器地址",
                placeholder: "https://sync.example.com",
                disabled: mode !== "self_hosted",
                hidden: mode !== "self_hosted",
            },
        },
        {
            key: "plaintextWarning",
            config: {
                type: "static" as const,
                label: "安全警告",
                value: "当前服务器地址使用 http:// 明文传输，同步数据（包含敏感配置）在传输过程中可能被窃听或篡改，生产环境请改用 https://。",
                hidden: mode !== "self_hosted" || !isPlaintextUrl,
            },
        },
        {
            key: "token",
            config: {
                type: "password" as const,
                label: status?.token_configured ? "访问 token（留空则沿用已保存 token）" : "访问 token",
                placeholder: status?.token_configured ? "已保存，可留空" : "Bearer token",
                disabled: mode !== "self_hosted",
                hidden: mode !== "self_hosted",
            },
        },
        {
            key: "autoSyncHint",
            config: {
                type: "static" as const,
                label: "自动同步",
                value: "已配置自建同步后，本地数据变更会在短暂防抖后自动推送，并定期/经服务器通知拉取远端变更。主操作互斥：后台自动同步中或本轮同步中只显示「停止同步」；已暂停或空闲时只显示「立即同步一次」（暂停时点它会恢复后台并跑一轮）。「重试失败」仅在有失败队列项时出现，用于重试失败的推送项，不是第三种同步模式。",
                hidden: mode !== "self_hosted",
            },
        },
        {
            key: "syncStatusRow",
            config: {
                type: "custom" as const,
                label: "",
                customRender: renderSyncStatusRow,
            },
        },
        {
            key: "syncActions",
            config: {
                type: "custom" as const,
                label: "同步操作",
                customRender: renderSyncActionButtons,
            },
        },
        {
            key: "needsResetWarning",
            config: {
                type: "static" as const,
                label: "同步状态待重置",
                value: "检测到服务器地址或访问 token 已变更，本机数据需要先与新服务器重新对齐后才能继续同步。",
                hidden: !status?.needs_reset,
            },
        },
        {
            key: "resetSyncState",
            config: {
                type: "button" as const,
                label: "重置同步状态",
                value: resetting ? "重置中..." : "重置并重新全量同步",
                hidden: !status?.needs_reset,
                disabled: mode !== "self_hosted" || resetting || saving,
                onClick: () => setResetConfirmOpen(true),
            },
        },
    ];

    return (
        <>
            <ConfigForm
                title="数据目录"
                description="管理本地数据目录与自建同步。"
                config={dataFolderConfig}
                layout="default"
                classNames="bottom-space"
                useFormReturn={form}
                onSave={handleSave}
            />
            <ConfirmDialog
                isOpen={resetConfirmOpen}
                title="重置同步状态"
                confirmText="将清空本机的同步游标、对象映射与待推送队列，然后与新服务器重新全量同步。本机数据本身不会被删除，但未推送的本地修改会以全量方式重新上传。确认继续？"
                onConfirm={handleResetSyncState}
                onCancel={() => setResetConfirmOpen(false)}
            />
            <ConfirmDialog
                isOpen={cleanupConfirmOpen}
                title="清理失效引用"
                confirmText={`将把本机约 ${danglingCount} 处悬空外键（如 llm_model_id、message_id 等指向已不存在行的字段）置为空值。同步推送时本就会按空值处理；此操作会改写本地数据库，请确认后继续。`}
                onConfirm={handleCleanupDanglingFks}
                onCancel={() => setCleanupConfirmOpen(false)}
            />
        </>
    );
};

export default React.memo(DataFolderConfigForm);
