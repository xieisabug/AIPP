import React, { useCallback, useEffect, useMemo, useState } from "react";
import { UseFormReturn, useWatch } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ConfigForm from "@/components/ConfigForm";
import ConfirmDialog from "@/components/ConfirmDialog";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
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
    formatLastSyncAt,
    formatRecentSyncError,
    formatSyncConnectionState,
    formatSyncStatusMetrics,
    friendlySyncError,
    type SyncStatusMetric,
    isAutoSyncEnabled,
    saveThenSyncSuccessMessage,
    shouldTriggerSyncAfterSave,
    showsSelfHostedSyncControls,
    syncConflictNotice,
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
    dead_letter_summary?: string | null;
    needs_reset: boolean;
    conflict_resolved_count: number;
    server_cursor: number;
    dangling_fk_count?: number;
    dangling_fk_summary?: string | null;
}

const STATUS_FALLBACK_POLL_MS = 30_000;
// Survives settings panel remounts; lifetime counters are never fresh conflicts.
const conflictNotices = new Map<string, { count: number; pending: number }>();

const SYNC_ACTIONS_HINT =
    "开启自动同步后，本地变更会防抖推送并定期/经服务器通知拉取。关闭后后台 worker 持久暂停；仍可用「立即同步一次」手动跑一轮。无论自动开或关，同步进行中都可「停止同步」取消本轮（不会关自动同步）。「立即同步一次」也会重试失败项。";

function connectionStateDotClass(state: string): string {
    switch (state) {
        case "已连接":
            return "bg-emerald-500";
        case "同步中":
            return "bg-primary";
        case "未连接":
            return "bg-amber-500";
        default:
            return "bg-muted-foreground";
    }
}

function metricValueClass(metric: SyncStatusMetric): string {
    if (metric.value > 0 && (metric.label === "失败" || metric.label === "无法应用")) {
        return "text-destructive";
    }
    return "text-foreground";
}

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({ form }) => {
    const [status, setStatus] = useState<SyncStatus | null>(null);
    const [statusReadError, setStatusReadError] = useState<string | null>(null);
    const [saving, setSaving] = useState(false);
    const [syncing, setSyncing] = useState(false);
    const [stopping, setStopping] = useState(false);
    const [togglingAutoSync, setTogglingAutoSync] = useState(false);
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

    const acceptStatus = useCallback((next: SyncStatus) => {
        const notice = syncConflictNotice(conflictNotices.get(next.server_url), next.conflict_resolved_count, next.syncing);
        conflictNotices.set(next.server_url, notice.state);
        setStatus(next);
        setStatusReadError(null);
        if (notice.delta > 0) {
            toast.warning(`有 ${notice.delta} 处修改与服务器冲突，已采用服务器版本`, { id: "sync-conflicts" });
        }
    }, []);

    const loadStatus = useCallback(async (syncFormValues = false) => {
        try {
            const nextStatus = await invoke<SyncStatus>("get_sync_status");
            acceptStatus(nextStatus);
            if (syncFormValues) {
                form.reset({
                    mode: nextStatus.mode || "local",
                    server_url: nextStatus.server_url || "",
                    token: "",
                });
            }
        } catch (error) {
            setStatusReadError(friendlySyncError(getErrorMessage(error), "读取同步状态失败"));
        }
    }, [form, acceptStatus]);

    useEffect(() => {
        void loadStatus(true);
        const unlistenPromise = listen<SyncStatus>("sync_status_changed", (event) => {
            acceptStatus(event.payload);
        });
        return () => {
            void unlistenPromise.then((unlisten) => unlisten());
        };
    }, [loadStatus, acceptStatus]);

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
            toast.success(
                status?.paused ? "已触发一次同步（自动同步已关闭）" : "已触发一次同步",
            );
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "触发同步失败"));
        } finally {
            setSyncing(false);
        }
    }, [form, persistSyncSettings, status?.paused, status?.server_url, status?.token_configured]);

    const handleStopSync = useCallback(async () => {
        setStopping(true);
        try {
            const nextStatus = await invoke<SyncStatus>("stop_sync");
            setStatus(nextStatus);
            toast.success("已停止当前同步");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "停止同步失败"));
        } finally {
            setStopping(false);
        }
    }, []);

    const handleSetAutoSync = useCallback(async (enabled: boolean) => {
        if (mode !== "self_hosted") {
            toast.error("请先切换到「自建同步」并保存");
            return;
        }
        setTogglingAutoSync(true);
        try {
            const nextStatus = await invoke<SyncStatus>("set_auto_sync", { enabled });
            setStatus(nextStatus);
            toast.success(enabled ? "已开启自动同步" : "已关闭自动同步");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "切换自动同步失败"));
        } finally {
            setTogglingAutoSync(false);
        }
    }, [mode]);

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
            if (nextStatus.dead_letter_count > 0) {
                toast.warning(`重试后仍有 ${nextStatus.dead_letter_count} 条无法应用，请查看具体原因`, { id: "sync-dead-letters" });
            } else {
                toast.success("无法应用的变更已全部处理");
            }
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

    const connectionState = useMemo(() => formatSyncConnectionState(status), [status]);
    const statusMetrics = useMemo(() => formatSyncStatusMetrics(status), [status]);
    const danglingText = useMemo(() => formatDanglingFkStatusText(status), [status]);
    const danglingCount = status?.dangling_fk_count ?? 0;
    const autoSyncOn = isAutoSyncEnabled({ mode, paused: status?.paused });
    const primaryControlInput = {
        mode,
        // Stop whenever a round is in flight (backend syncing or local Sync Now click).
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
        const summaryItems = [
            { label: "上次同步时间", value: formatLastSyncAt(status?.last_sync_at) },
            {
                label: "最近错误",
                value: formatRecentSyncError({
                    last_error: status?.last_error,
                    failed_outbox_count: status?.failed_outbox_count,
                    syncing: Boolean(status?.syncing) || syncing,
                }),
            },
        ];
        return (
            <div className="space-y-3">
                <div className="min-w-0 space-y-2">
                    <div className="font-semibold text-sm text-foreground">连接状态</div>
                    <div className="px-3 py-2 bg-muted rounded-md">
                        <div className="flex flex-wrap items-center gap-2">
                            <div className="flex items-center gap-2 text-sm font-medium text-foreground">
                                <span
                                    className={`inline-block h-1.5 w-1.5 shrink-0 rounded-full ${connectionStateDotClass(connectionState)}`}
                                    aria-hidden
                                />
                                {connectionState}
                            </div>
                            {statusMetrics.map((metric) => (
                                <span
                                    key={metric.label}
                                    className="inline-flex items-center gap-1 rounded-md bg-background px-2 py-1 text-xs"
                                >
                                    <span className="text-muted-foreground">{metric.label}</span>
                                    <span className={`font-medium tabular-nums ${metricValueClass(metric)}`}>
                                        {metric.value.toLocaleString("zh-CN")}
                                    </span>
                                </span>
                            ))}
                        </div>
                    </div>
                </div>
                <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                    {summaryItems.map((item) => (
                        <div key={item.label} className="min-w-0 space-y-2">
                            <div className="font-semibold text-sm text-foreground">{item.label}</div>
                            <div className="text-sm text-muted-foreground px-3 py-2 bg-muted rounded-md break-words whitespace-pre-wrap">
                                {item.value}
                            </div>
                        </div>
                    ))}
                </div>
                {statusReadError ? <p className="text-sm text-destructive break-words">{statusReadError}</p> : null}
                {status?.dead_letter_summary ? (
                    <details className="text-sm text-destructive break-words">
                        <summary className="cursor-pointer">无法应用的原因（{status.dead_letter_count} 条）</summary>
                        <p className="mt-2 whitespace-pre-wrap">{status.dead_letter_summary}</p>
                    </details>
                ) : null}
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
    }, [connectionState, danglingCount, danglingText, statusReadError, status?.dead_letter_count, status?.dead_letter_summary, status?.failed_outbox_count, status?.last_error, status?.last_sync_at, status?.syncing, statusMetrics, syncing]);

    const renderSyncActionButtons = useCallback(() => {
        // Auto-sync toggle is the persistent on/off. Stop shows while any round is syncing.
        // 「同步中」lives only in the connection status row, not as a button label.
        return (
            <div className="space-y-3">
                <div className="flex flex-wrap items-center gap-3">
                    <div className="flex items-center gap-2">
                        <Switch
                            checked={autoSyncOn}
                            disabled={togglingAutoSync || saving || mode !== "self_hosted"}
                            onCheckedChange={(value) => {
                                void handleSetAutoSync(value);
                            }}
                        />
                        <span className="text-sm text-foreground">
                            {autoSyncOn ? "自动同步：开" : "自动同步：关"}
                        </span>
                    </div>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                    {showSyncNow ? (
                        <Button
                            type="button"
                            variant="outline"
                            className="hover:bg-muted hover:border-border"
                            disabled={syncing || saving || stopping || togglingAutoSync || Boolean(status?.syncing)}
                            onClick={handleSyncNow}
                        >
                            {status?.token_configured ? "立即同步一次" : "保存并同步"}
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
            </div>
        );
    }, [
        autoSyncOn,
        cleaningDangling,
        handleRetryDeadLetters,
        handleRetryFailed,
        handleSetAutoSync,
        handleStopSync,
        handleSyncNow,
        mode,
        retryingDeadLetters,
        retryingFailed,
        saving,
        showCleanupDangling,
        showRetryDeadLetters,
        showRetryFailed,
        showSyncNow,
        status?.syncing,
        status?.token_configured,
        stopEnabled,
        stopping,
        syncing,
        togglingAutoSync,
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
                hint: isPlaintextUrl
                    ? "当前地址使用 http:// 明文传输，同步数据可能被窃听或篡改，生产环境请改用 https://。"
                    : undefined,
            },
        },
        {
            key: "token",
            config: {
                type: "password" as const,
                label: "访问 token",
                placeholder: status?.token_configured ? "留空则沿用已保存 token" : "Bearer token",
                disabled: mode !== "self_hosted",
                hidden: mode !== "self_hosted",
            },
        },
        {
            key: "syncStatusRow",
            config: {
                type: "custom" as const,
                label: "",
                hidden: !showsSelfHostedSyncControls(mode),
                customRender: renderSyncStatusRow,
            },
        },
        {
            key: "syncActions",
            config: {
                type: "custom" as const,
                label: "同步操作",
                tooltip: SYNC_ACTIONS_HINT,
                hidden: !showsSelfHostedSyncControls(mode),
                customRender: renderSyncActionButtons,
            },
        },
        {
            key: "needsResetWarning",
            config: {
                type: "static" as const,
                label: "同步账号范围已变更",
                value: "服务器地址或访问 token 已变更。请先备份，再使用独立数据目录连接；重置无法安全合并两份已有历史。",
                hidden: !showsSelfHostedSyncControls(mode) || !status?.needs_reset,
            },
        },
        {
            key: "resetSyncState",
            config: {
                type: "button" as const,
                label: "重置同步状态",
                value: resetting ? "重置中..." : "重置并重新全量同步",
                hidden: !showsSelfHostedSyncControls(mode) || !!status?.needs_reset,
                disabled: resetting || saving,
                onClick: () => setResetConfirmOpen(true),
            },
        },
    ];

    return (
        <>
            <ConfigForm
                title="数据"
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
                confirmText="将从头重新读取当前服务器的同步记录，保留对象身份、已确认版本和待上传修改，不会把历史会话重新生成为新对象。此操作仅适用于原服务器和账号；更换账号请先备份并使用独立数据目录。确认继续？"
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
