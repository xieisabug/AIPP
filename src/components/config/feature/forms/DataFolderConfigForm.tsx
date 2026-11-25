import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Controller, UseFormReturn, useWatch } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ChevronDown, FolderOpen } from "lucide-react";
import { SyncServerSwitchDialog } from "./SyncServerSwitchDialog";
import ConfirmDialog from "@/components/ConfirmDialog";
import { ConfigSaveFooter } from "@/components/common/ConfigSaveFooter";
import { SettingsGroup, SettingRowText, settingRowClassName, stackedRowClassName } from "@/components/config/SettingsGroup";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { PasswordInput } from "@/components/ui/password-input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Switch } from "@/components/ui/switch";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { useUnsavedLeaveGuard } from "@/hooks/useUnsavedLeaveGuard";
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
    secretSyncTone,
    shouldTriggerSyncAfterSave,
    showsSelfHostedSyncControls,
    syncConflictNotice,
    isSyncPassphraseMismatch,
    providerConflictNotice,
    validateSyncPassphrase,
} from "./dataFolderSyncHelpers";

interface DataFolderConfigFormProps {
    form: UseFormReturn<any>;
    /** Register unsaved-change leave guard when mounted inside the top-level Data & Sync page. */
    enableLeaveGuard?: boolean;
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
    secret_sync_state?: string;
    secret_sync_message?: string;
    secret_pending_count?: number;
    duplicate_provider_conflicts?: string[];
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

function secretToneDotClass(tone: ReturnType<typeof secretSyncTone>): string {
    switch (tone) {
        case "ok":
            return "bg-emerald-500";
        case "warn":
            return "bg-amber-500";
        default:
            return "bg-muted-foreground";
    }
}

const PASSPHRASE_FIELD = "sync_passphrase";

const SECRET_SYNC_HINT =
    "API Key 与应用连接凭据共用同步口令加密后上传，服务器只保存密文。未设置口令时，应用凭据使用本机自动生成的密钥加密保存；设置或更换口令时自动重新加密。每台设备输入相同口令才能解密；口令不会上传。原加密密钥丢失时，仅设置新口令无法恢复旧凭据。";

function metricValueClass(metric: SyncStatusMetric): string {
    if (metric.value > 0 && (metric.label === "失败" || metric.label === "无法应用")) {
        return "text-destructive";
    }
    return "text-foreground";
}

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({
    form,
    enableLeaveGuard = false,
}) => {
    const isMobilePlatform = useIsMobilePlatform();
    const statusRevision = useRef(0);
    const initialFormPending = useRef(true);
    const [status, setStatus] = useState<SyncStatus | null>(null);
    const [statusReadError, setStatusReadError] = useState<string | null>(null);
    const [serverSwitch, setServerSwitch] = useState<{
        id: string;
        oldUrl: string;
        serverUrl: string;
        initialToken: string;
        initialChoice?: "new" | "credentials";
    } | null>(null);
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
    const [passphraseError, setPassphraseError] = useState<string | null>(null);
    const [savingPassphrase, setSavingPassphrase] = useState(false);
    const [replaceConfirmOpen, setReplaceConfirmOpen] = useState(false);
    const [maintenanceOpen, setMaintenanceOpen] = useState(false);
    const maintenanceUserToggled = useRef(false);

    const watchedMode = useWatch({ control: form.control, name: "mode" });
    const mode = watchedMode || "local";
    const watchedServerUrl = useWatch({ control: form.control, name: "server_url" }) || "";
    const watchedToken = useWatch({ control: form.control, name: "token" }) || "";
    const isPlaintextUrl = watchedServerUrl.trim().toLowerCase().startsWith("http://");

    const savedMode = status?.mode || "local";
    const savedServerUrl = status?.server_url || "";
    const hasUnsavedConnectionChanges = Boolean(
        status &&
            (mode !== savedMode ||
                (mode === "self_hosted" && (watchedServerUrl !== savedServerUrl || Boolean(watchedToken))))
    );

    const acceptStatus = useCallback(
        (next: SyncStatus) => {
            statusRevision.current += 1;
            const notice = syncConflictNotice(
                conflictNotices.get(next.server_url),
                next.conflict_resolved_count,
                next.syncing
            );
            conflictNotices.set(next.server_url, notice.state);
            setStatus(next);
            if (initialFormPending.current) {
                initialFormPending.current = false;
                form.reset({ mode: next.mode || "local", server_url: next.server_url || "", token: "" });
            }
            setStatusReadError(null);
            if (notice.delta > 0) {
                toast.warning(`有 ${notice.delta} 处修改与服务器冲突，已采用服务器版本`, { id: "sync-conflicts" });
            }
        },
        [form]
    );

    const loadStatus = useCallback(
        async (syncFormValues = false) => {
            const revision = ++statusRevision.current;
            try {
                const nextStatus = await invoke<SyncStatus>("get_sync_status");
                if (revision !== statusRevision.current) return;
                acceptStatus(nextStatus);
                if (syncFormValues) {
                    form.reset({
                        mode: nextStatus.mode || "local",
                        server_url: nextStatus.server_url || "",
                        token: "",
                    });
                }
            } catch (error) {
                if (revision !== statusRevision.current) return;
                setStatusReadError(friendlySyncError(getErrorMessage(error), "读取同步状态失败"));
            }
        },
        [form, acceptStatus]
    );

    useEffect(() => {
        void loadStatus(true);
        const unlistenPromise = listen<SyncStatus>("sync_status_changed", (event) => {
            acceptStatus(event.payload);
        });
        return () => {
            statusRevision.current += 1;
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
        window.addEventListener("online", refresh);
        window.addEventListener("aipp:mobile-navigate", refresh);
        document.addEventListener("visibilitychange", refreshWhenVisible);
        return () => {
            window.removeEventListener("focus", refresh);
            window.removeEventListener("online", refresh);
            window.removeEventListener("aipp:mobile-navigate", refresh);
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

    const persistSyncSettings = useCallback(
        async (opts?: { quietSuccess?: boolean }) => {
            const values = form.getValues();
            const nextStatus = await invoke<SyncStatus>("save_sync_settings", {
                request: {
                    mode: values.mode || "local",
                    server_url: values.server_url || "",
                    token: values.token || "",
                },
            });
            acceptStatus(nextStatus);
            form.reset({
                mode: nextStatus.mode || "local",
                server_url: nextStatus.server_url || "",
                token: "",
            });
            if (!opts?.quietSuccess) {
                toast.success(
                    nextStatus.mode === "self_hosted"
                        ? "自建同步配置已保存，后台将自动同步"
                        : "已切换为本地模式"
                );
            }
            if (nextStatus.mode === "self_hosted") {
                void loadStatus(false);
            }
            return nextStatus;
        },
        [form, loadStatus, acceptStatus]
    );

    const beginServerSwitchIfNeeded = useCallback(async () => {
        const values = form.getValues();
        if (values.mode !== "self_hosted") return false;
        const normalize = (url: string) => url.trim().replace(/\/+$/, "");
        const first = !status?.token_configured;
        const changed = normalize(values.server_url || "") !== normalize(status?.server_url || "");
        if (!first && !changed && !values.token?.trim() && !status?.needs_reset) return false;
        const started = await invoke<{ id: string; old_url: string; has_sync_state: boolean }>(
            "begin_sync_server_switch"
        );
        if (started.has_sync_state && !changed && !status?.needs_reset) {
            try {
                const checked = await invoke<{ mode: string | null }>("inspect_sync_server_switch", {
                    request: {
                        id: started.id,
                        server_url: normalize(values.server_url || ""),
                        token: values.token || "",
                        choice: "credentials",
                    },
                });
                if (checked.mode) {
                    const next = await invoke<SyncStatus>("commit_sync_server_switch", {
                        id: started.id,
                        passphrase: null,
                    });
                    acceptStatus(next);
                    form.reset({ mode: next.mode, server_url: next.server_url, token: "" });
                    toast.success("访问 Token 已验证并保存，继续同步");
                    return true;
                }
            } catch (error) {
                await invoke("cancel_sync_server_switch", { id: started.id }).catch(() => {});
                throw error;
            }
        }
        setServerSwitch({
            id: started.id,
            oldUrl: started.old_url,
            serverUrl: normalize(values.server_url || ""),
            initialToken: values.token || "",
            initialChoice: !started.has_sync_state ? "new" : undefined,
        });
        return true;
    }, [form, status, acceptStatus]);

    const handleSave = useCallback(async () => {
        setSaving(true);
        try {
            if (await beginServerSwitchIfNeeded()) return;
            await persistSyncSettings();
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "保存同步配置失败"));
        } finally {
            setSaving(false);
        }
    }, [persistSyncSettings, beginServerSwitchIfNeeded]);

    const handleDiscard = useCallback(() => {
        if (!status) return;
        form.reset({
            mode: status.mode || "local",
            server_url: status.server_url || "",
            token: "",
        });
    }, [form, status]);

    const { dialog: leaveDialog } = useUnsavedLeaveGuard({
        isDirty: enableLeaveGuard && hasUnsavedConnectionChanges,
        onSave: handleSave,
        onDiscard: handleDiscard,
    });

    const handleSyncNow = useCallback(async () => {
        const values = form.getValues();
        const decision = decideSyncNowAction({
            formMode: values.mode || "local",
            formServerUrl: values.server_url || "",
            formToken: values.token || "",
            tokenConfigured: Boolean(status?.token_configured),
            savedServerUrl: status?.server_url || "",
        });

        if (
            decision.action === "wrong_mode" ||
            decision.action === "need_token" ||
            decision.action === "need_server_url"
        ) {
            toast.error(decision.message);
            return;
        }

        setSyncing(true);
        try {
            if (await beginServerSwitchIfNeeded()) return;
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
                    acceptStatus(nextStatus);
                } else {
                    acceptStatus(saved);
                }
                toast.success(saveThenSyncSuccessMessage(needSyncNow));
                return;
            }

            // sync_only: save did not run, so trigger sync now.
            const nextStatus = await invoke<SyncStatus>("trigger_sync_now");
            acceptStatus(nextStatus);
            toast.success(
                status?.paused ? "已触发一次同步（自动同步已关闭）" : "已触发一次同步"
            );
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "触发同步失败"));
        } finally {
            setSyncing(false);
        }
    }, [
        form,
        persistSyncSettings,
        beginServerSwitchIfNeeded,
        status?.paused,
        status?.server_url,
        status?.token_configured,
        acceptStatus,
    ]);

    const handleStopSync = useCallback(async () => {
        setStopping(true);
        try {
            const nextStatus = await invoke<SyncStatus>("stop_sync");
            acceptStatus(nextStatus);
            toast.success("已停止当前同步");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "停止同步失败"));
        } finally {
            setStopping(false);
        }
    }, [acceptStatus]);

    const handleSetAutoSync = useCallback(
        async (enabled: boolean) => {
            if (mode !== "self_hosted") {
                toast.error("请先切换到「自建同步」并保存");
                return;
            }
            setTogglingAutoSync(true);
            try {
                const nextStatus = await invoke<SyncStatus>("set_auto_sync", { enabled });
                acceptStatus(nextStatus);
                toast.success(enabled ? "已开启自动同步" : "已关闭自动同步");
            } catch (error) {
                toast.error(friendlySyncError(getErrorMessage(error), "切换自动同步失败"));
            } finally {
                setTogglingAutoSync(false);
            }
        },
        [mode, acceptStatus]
    );

    const handleRetryFailed = useCallback(async () => {
        setRetryingFailed(true);
        try {
            const nextStatus = await invoke<SyncStatus>("retry_failed_sync_outbox");
            acceptStatus(nextStatus);
            toast.success("已重新触发失败项");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "重试失败项失败"));
        } finally {
            setRetryingFailed(false);
        }
    }, [acceptStatus]);

    const handleRetryDeadLetters = useCallback(async () => {
        setRetryingDeadLetters(true);
        try {
            const nextStatus = await invoke<SyncStatus>("retry_sync_dead_letters");
            acceptStatus(nextStatus);
            if (nextStatus.dead_letter_count > 0) {
                toast.warning(
                    `重试后仍有 ${nextStatus.dead_letter_count} 条无法应用，请查看具体原因`,
                    { id: "sync-dead-letters" }
                );
            } else {
                toast.success("无法应用的变更已全部处理");
            }
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "重试无法应用的变更失败"));
        } finally {
            setRetryingDeadLetters(false);
        }
    }, [acceptStatus]);

    const handleResetSyncState = useCallback(async () => {
        setResetConfirmOpen(false);
        setResetting(true);
        try {
            const nextStatus = await invoke<SyncStatus>("reset_sync_state");
            acceptStatus(nextStatus);
            toast.success("同步状态已重置，正在重新全量同步");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "重置同步状态失败"));
        } finally {
            setResetting(false);
        }
    }, [acceptStatus]);

    const handleCleanupDanglingFks = useCallback(async () => {
        setCleanupConfirmOpen(false);
        setCleaningDangling(true);
        try {
            const nextStatus = await invoke<SyncStatus>("cleanup_dangling_foreign_keys");
            acceptStatus(nextStatus);
            const notice = formatDanglingFkStatusText(nextStatus);
            toast.success(notice || "已清理失效的本地引用");
        } catch (error) {
            toast.error(friendlySyncError(getErrorMessage(error), "清理失效引用失败"));
        } finally {
            setCleaningDangling(false);
        }
    }, [acceptStatus]);

    const submitPassphrase = useCallback(
        async (replaceExisting: boolean) => {
            const passphrase: string = form.getValues(PASSPHRASE_FIELD) || "";
            const invalid = validateSyncPassphrase(passphrase);
            if (invalid) {
                setPassphraseError(invalid);
                return;
            }
            setPassphraseError(null);
            setSavingPassphrase(true);
            try {
                const nextStatus = await invoke<SyncStatus>("set_sync_passphrase", {
                    request: { passphrase, replace_existing: replaceExisting },
                });
                acceptStatus(nextStatus);
                form.setValue(PASSPHRASE_FIELD, "");
                toast.success(replaceExisting ? "已更换同步口令" : "同步口令已保存");
            } catch (error) {
                const message = getErrorMessage(error) || "设置同步口令失败";
                // One button: a different passphrase than the account's means "replace",
                // which only happens after the user confirms it.
                if (!replaceExisting && isSyncPassphraseMismatch(message)) {
                    setReplaceConfirmOpen(true);
                    return;
                }
                // Passphrase errors are already user-facing; do not route them through the
                // generic "解密" → token hint mapping.
                setPassphraseError(message);
                toast.error(message);
            } finally {
                setSavingPassphrase(false);
            }
        },
        [form, acceptStatus]
    );

    const connectionState = useMemo(() => formatSyncConnectionState(status), [status]);
    const statusMetrics = useMemo(() => formatSyncStatusMetrics(status), [status]);
    const danglingText = useMemo(() => formatDanglingFkStatusText(status), [status]);
    const danglingCount = status?.dangling_fk_count ?? 0;
    const autoSyncOn = isAutoSyncEnabled({ mode, paused: status?.paused });
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
    const showSelfHosted = showsSelfHostedSyncControls(mode);

    const hasMaintenanceIssues = Boolean(
        failedOutboxCount > 0 || deadLetterCount > 0 || danglingCount > 0 || status?.needs_reset
    );

    useEffect(() => {
        if (!maintenanceUserToggled.current && hasMaintenanceIssues) {
            setMaintenanceOpen(true);
        }
    }, [hasMaintenanceIssues]);

    const secretState = status?.secret_sync_state;
    const secretTone = secretSyncTone(secretState);
    const conflictNotice = providerConflictNotice(status?.duplicate_provider_conflicts);

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
        <>
            {serverSwitch && (
                <SyncServerSwitchDialog
                    key={serverSwitch.id}
                    {...serverSwitch}
                    onCancel={() => setServerSwitch(null)}
                    onComplete={(next: SyncStatus) => {
                        acceptStatus(next);
                        form.reset({ mode: next.mode, server_url: next.server_url, token: "" });
                        setServerSwitch(null);
                        toast.success("新连接配置已保存，正在同步；进度和错误请查看同步状态");
                    }}
                />
            )}

            <div className="space-y-6">
                {/* 卡片 1：本地数据（移动端隐藏） */}
                {!isMobilePlatform ? (
                    <SettingsGroup
                        id="local-data"
                        title="本地数据"
                        description="AIPP 的对话历史、助手配置与知识库均保存在本机 SQLite 数据库中。"
                    >
                        <div className={settingRowClassName}>
                            <SettingRowText
                                label="数据文件夹"
                                description="在系统文件管理器中打开本地应用数据目录，便于手动备份或迁移。"
                            />
                            <Button
                                type="button"
                                variant="outline"
                                size="sm"
                                className="shrink-0 gap-1.5"
                                onClick={() => void handleOpenDataFolder()}
                            >
                                <FolderOpen className="h-4 w-4" />
                                打开
                            </Button>
                        </div>
                    </SettingsGroup>
                ) : null}

                {/* 卡片 2：跨设备同步 */}
                <SettingsGroup
                    id="sync-connection"
                    title="跨设备同步"
                    description="连接自建 AIPP sync-server 实现多设备实时同步，或保持仅本机存储。"
                >
                    <div className={stackedRowClassName}>
                        <SettingRowText label="同步模式" />
                        <Controller
                            control={form.control}
                            name="mode"
                            render={({ field }) => (
                                <RadioGroup
                                    value={field.value || "local"}
                                    onValueChange={field.onChange}
                                    className="grid grid-cols-1 gap-2 sm:grid-cols-2"
                                >
                                    <Label
                                        htmlFor="sync-mode-local"
                                        className="flex cursor-pointer items-start gap-2.5 rounded-md border p-3 font-normal hover:bg-muted/40"
                                    >
                                        <RadioGroupItem value="local" id="sync-mode-local" className="mt-0.5" />
                                        <span className="space-y-0.5">
                                            <span className="block text-sm font-medium text-foreground">本地模式</span>
                                            <span className="block text-xs text-muted-foreground">
                                                只使用本机 SQLite，并停止后台同步。
                                            </span>
                                        </span>
                                    </Label>
                                    <Label
                                        htmlFor="sync-mode-self-hosted"
                                        className="flex cursor-pointer items-start gap-2.5 rounded-md border p-3 font-normal hover:bg-muted/40"
                                    >
                                        <RadioGroupItem
                                            value="self_hosted"
                                            id="sync-mode-self-hosted"
                                            className="mt-0.5"
                                        />
                                        <span className="space-y-0.5">
                                            <span className="block text-sm font-medium text-foreground">自建同步</span>
                                            <span className="block text-xs text-muted-foreground">
                                                连接你自建的 AIPP sync-server，改动自动同步。
                                            </span>
                                        </span>
                                    </Label>
                                </RadioGroup>
                            )}
                        />
                    </div>

                    <div className={showSelfHosted ? stackedRowClassName : "hidden"}>
                        <SettingRowText label="服务器地址" />
                        <Controller
                            control={form.control}
                            name="server_url"
                            render={({ field }) => (
                                <Input
                                    type="url"
                                    inputMode="url"
                                    placeholder="https://sync.example.com"
                                    disabled={!showSelfHosted}
                                    value={field.value ?? ""}
                                    onChange={(e) => field.onChange(e.target.value)}
                                    onBlur={field.onBlur}
                                    name={field.name}
                                    ref={field.ref}
                                />
                            )}
                        />
                        {isPlaintextUrl ? (
                            <p className="text-xs text-amber-600 dark:text-amber-400">
                                当前地址使用 http:// 明文传输，同步数据可能被窃听或篡改，生产环境请改用 https://。
                            </p>
                        ) : null}
                    </div>

                    <div className={showSelfHosted ? stackedRowClassName : "hidden"}>
                        <SettingRowText label="访问 token" />
                        <Controller
                            control={form.control}
                            name="token"
                            render={({ field }) => (
                                <PasswordInput
                                    autoComplete="off"
                                    placeholder={
                                        status?.token_configured ? "留空则沿用已保存 token" : "Bearer token"
                                    }
                                    disabled={!showSelfHosted}
                                    value={field.value ?? ""}
                                    onChange={(e) => field.onChange(e.target.value)}
                                    onBlur={field.onBlur}
                                    name={field.name}
                                    ref={field.ref}
                                />
                            )}
                        />
                    </div>

                    {showSelfHosted ? (
                        <>
                            <div className={stackedRowClassName}>
                                <div className="space-y-3">
                                    <div className="min-w-0 space-y-2">
                                        <div className="text-sm font-medium text-foreground">连接状态</div>
                                        <div className="rounded-md bg-muted px-3 py-2">
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
                                                        <span
                                                            className={`font-medium tabular-nums ${metricValueClass(metric)}`}
                                                        >
                                                            {metric.value.toLocaleString("zh-CN")}
                                                        </span>
                                                    </span>
                                                ))}
                                            </div>
                                        </div>
                                    </div>
                                    <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                                        {summaryItems.map((item) => (
                                            <div key={item.label} className="min-w-0 space-y-1.5">
                                                <div className="text-xs font-medium text-muted-foreground">
                                                    {item.label}
                                                </div>
                                                <div className="break-words whitespace-pre-wrap rounded-md bg-muted px-3 py-2 text-sm text-muted-foreground">
                                                    {item.value}
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                    {statusReadError ? (
                                        <p className="break-words text-sm text-destructive">{statusReadError}</p>
                                    ) : null}
                                    {status?.dead_letter_summary ? (
                                        <details className="break-words text-sm text-destructive">
                                            <summary className="cursor-pointer">
                                                无法应用的原因（{status.dead_letter_count} 条）
                                            </summary>
                                            <p className="mt-2 whitespace-pre-wrap">{status.dead_letter_summary}</p>
                                        </details>
                                    ) : null}
                                    {danglingText ? (
                                        <div className="break-words rounded-md border border-border bg-muted px-3 py-2 text-sm text-foreground">
                                            {danglingText}
                                            {danglingCount > 0
                                                ? " 如需把本机对应字段也清空，可在下方「故障排查与维护」中点击「清理失效引用」。"
                                                : ""}
                                        </div>
                                    ) : null}
                                </div>
                            </div>

                            <div className={stackedRowClassName}>
                                <div className="flex flex-wrap items-center justify-between gap-3">
                                    <SettingRowText label="同步操作" description={SYNC_ACTIONS_HINT} />
                                    <div className="flex shrink-0 items-center gap-2">
                                        <Switch
                                            aria-label="自动同步"
                                            checked={autoSyncOn}
                                            disabled={togglingAutoSync || saving || mode !== "self_hosted"}
                                            onCheckedChange={(value) => void handleSetAutoSync(value)}
                                        />
                                        <span className="text-sm text-foreground">
                                            {autoSyncOn ? "自动同步：开" : "自动同步：关"}
                                        </span>
                                    </div>
                                </div>
                                <div className="flex flex-wrap items-center gap-2 pt-1">
                                    {showSyncNow ? (
                                        <Button
                                            type="button"
                                            variant="outline"
                                            className="hover:border-border hover:bg-muted"
                                            disabled={
                                                syncing ||
                                                saving ||
                                                stopping ||
                                                togglingAutoSync ||
                                                Boolean(status?.syncing)
                                            }
                                            onClick={handleSyncNow}
                                        >
                                            {status?.token_configured ? "立即同步一次" : "保存并同步"}
                                        </Button>
                                    ) : null}
                                    {stopEnabled ? (
                                        <Button
                                            type="button"
                                            variant="outline"
                                            className="hover:border-border hover:bg-muted"
                                            disabled={stopping || saving}
                                            onClick={handleStopSync}
                                        >
                                            {stopping ? "停止中..." : "停止同步"}
                                        </Button>
                                    ) : null}
                                </div>
                            </div>
                        </>
                    ) : null}
                </SettingsGroup>

                {/* 卡片 3：端到端凭据加密（local 模式下用 hidden 类保留挂载以兼容现有行为） */}
                <SettingsGroup
                    id="passphrase"
                    title="端到端凭据加密"
                    description={SECRET_SYNC_HINT}
                    className={showSelfHosted ? undefined : "hidden"}
                >
                    <div className={stackedRowClassName}>
                        <div
                            className="flex items-start gap-2 rounded-md bg-muted px-3 py-2 text-sm"
                            data-testid="secret-sync-status"
                        >
                            <span
                                className={`mt-1.5 inline-block h-1.5 w-1.5 shrink-0 rounded-full ${secretToneDotClass(secretTone)}`}
                                aria-hidden
                            />
                            <span
                                className={`min-w-0 break-words text-foreground ${secretTone === "warn" ? "font-medium" : ""}`}
                            >
                                {status?.secret_sync_message || "读取中"}
                            </span>
                        </div>
                        <Controller
                            control={form.control}
                            name={PASSPHRASE_FIELD}
                            render={({ field }) => {
                                const passphrase = field.value || "";
                                const busy = savingPassphrase || saving;
                                return (
                                    <div className="flex flex-col gap-2 md:flex-row md:items-center">
                                        {isMobilePlatform ? (
                                            <PasswordInput
                                                autoComplete="new-password"
                                                aria-label="同步口令"
                                                className="w-full md:max-w-xs"
                                                placeholder="同步口令（至少 8 个字符，所有设备相同）"
                                                value={passphrase}
                                                disabled={busy}
                                                onChange={(event) => field.onChange(event.target.value)}
                                                onKeyDown={(event) => {
                                                    if (event.key === "Enter") {
                                                        event.preventDefault();
                                                        void submitPassphrase(false);
                                                    }
                                                }}
                                            />
                                        ) : (
                                            <Input
                                                type="password"
                                                autoComplete="new-password"
                                                aria-label="同步口令"
                                                className="w-full md:max-w-xs"
                                                placeholder="同步口令（至少 8 个字符，所有设备相同）"
                                                value={passphrase}
                                                disabled={busy}
                                                onChange={(event) => field.onChange(event.target.value)}
                                                onKeyDown={(event) => {
                                                    if (event.key === "Enter") {
                                                        event.preventDefault();
                                                        void submitPassphrase(false);
                                                    }
                                                }}
                                            />
                                        )}
                                        <Button
                                            type="button"
                                            variant="outline"
                                            className="hover:border-border hover:bg-muted"
                                            disabled={busy || !passphrase}
                                            onClick={() => void submitPassphrase(false)}
                                        >
                                            {savingPassphrase ? "处理中..." : "保存口令"}
                                        </Button>
                                    </div>
                                );
                            }}
                        />
                        {passphraseError ? (
                            <p className="break-words text-sm text-destructive">{passphraseError}</p>
                        ) : null}
                        {conflictNotice ? (
                            <p
                                className="break-words text-sm font-medium text-foreground"
                                data-testid="provider-conflict-status"
                            >
                                {conflictNotice}
                            </p>
                        ) : null}
                    </div>
                </SettingsGroup>

                {/* 卡片 4：故障排查与维护（默认折叠，有异常自动展开） */}
                {showSelfHosted ? (
                    <section id="sync-maintenance" className="rounded-lg border">
                        <button
                            type="button"
                            className="flex w-full items-center justify-between gap-3 px-4 py-3 text-left hover:bg-muted/40"
                            aria-expanded={maintenanceOpen}
                            onClick={() => {
                                maintenanceUserToggled.current = true;
                                setMaintenanceOpen((prev) => !prev);
                            }}
                        >
                            <div className="min-w-0">
                                <div className="flex items-center gap-2 text-sm font-medium">
                                    <span>故障排查与维护</span>
                                    {hasMaintenanceIssues ? (
                                        <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] font-medium text-amber-700 dark:text-amber-300">
                                            需要注意
                                        </span>
                                    ) : null}
                                </div>
                                <p className="mt-0.5 text-xs text-muted-foreground">
                                    仅在遇到同步失败、无法应用的变更、悬空引用或需要重新全量拉取时使用。
                                </p>
                            </div>
                            <ChevronDown
                                className={`h-4 w-4 shrink-0 text-muted-foreground transition-transform ${
                                    maintenanceOpen ? "rotate-180" : ""
                                }`}
                            />
                        </button>

                        {maintenanceOpen ? (
                            <div className="divide-y border-t">
                                {status?.needs_reset ? (
                                    <div className={stackedRowClassName}>
                                        <SettingRowText
                                            label="同步账号范围已变更"
                                            description="请点击底部「保存配置」或「立即同步一次」，选择服务器迁移或新同步服务器后继续。"
                                        />
                                    </div>
                                ) : null}

                                {showRetryFailed || showRetryDeadLetters || showCleanupDangling ? (
                                    <div className={stackedRowClassName}>
                                        <SettingRowText
                                            label="异常项处理"
                                            description="针对当前待处理的失败推送、无法应用的远端变更或本地失效引用执行修复。"
                                        />
                                        <div className="flex flex-wrap items-center gap-2 pt-1">
                                            {showRetryFailed ? (
                                                <Button
                                                    type="button"
                                                    variant="outline"
                                                    className="hover:border-border hover:bg-muted"
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
                                                    className="hover:border-border hover:bg-muted"
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
                                                    className="hover:border-border hover:bg-muted"
                                                    disabled={cleaningDangling || saving}
                                                    onClick={() => setCleanupConfirmOpen(true)}
                                                >
                                                    {cleaningDangling ? "清理中..." : "清理失效引用"}
                                                </Button>
                                            ) : null}
                                        </div>
                                    </div>
                                ) : null}

                                {!status?.needs_reset ? (
                                    <div className={settingRowClassName}>
                                        <SettingRowText
                                            label="重置同步状态"
                                            description="从头重新读取当前服务器的同步记录，保留对象身份和待上传修改。"
                                        />
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            disabled={resetting || saving}
                                            onClick={() => setResetConfirmOpen(true)}
                                        >
                                            {resetting ? "重置中..." : "重置并重新全量同步"}
                                        </Button>
                                    </div>
                                ) : null}
                            </div>
                        ) : null}
                    </section>
                ) : null}
            </div>

            <ConfigSaveFooter>
                <div className="flex items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                        <Button type="button" disabled={saving} onClick={() => void handleSave()}>
                            {saving ? "保存中..." : "保存配置"}
                        </Button>
                        {hasUnsavedConnectionChanges ? (
                            <Button
                                type="button"
                                variant="outline"
                                disabled={saving}
                                onClick={handleDiscard}
                            >
                                放弃更改
                            </Button>
                        ) : null}
                    </div>
                    {hasUnsavedConnectionChanges ? (
                        <span className="text-xs text-muted-foreground">有未保存的连接设置</span>
                    ) : null}
                </div>
            </ConfigSaveFooter>

            <ConfirmDialog
                isOpen={resetConfirmOpen}
                title="重置同步状态"
                confirmText="将从头重新读取当前服务器的同步记录，保留对象身份、已确认版本和待上传修改，不会把历史会话重新生成为新对象。此操作仅适用于原服务器和账号；更换账号请先备份并使用独立数据目录。确认继续？"
                onConfirm={handleResetSyncState}
                onCancel={() => setResetConfirmOpen(false)}
            />
            <ConfirmDialog
                isOpen={replaceConfirmOpen}
                title="更换同步口令"
                confirmText="输入的口令与已有同步口令不一致。继续将更换所有设备的同步口令，其他设备需要重新输入新口令。"
                onConfirm={() => {
                    setReplaceConfirmOpen(false);
                    void submitPassphrase(true);
                }}
                onCancel={() => setReplaceConfirmOpen(false)}
            />
            <ConfirmDialog
                isOpen={cleanupConfirmOpen}
                title="清理失效引用"
                confirmText={`将把本机约 ${danglingCount} 处悬空外键（如 llm_model_id、message_id 等指向已不存在行的字段）置为空值。同步推送时本就会按空值处理；此操作会改写本地数据库，请确认后继续。`}
                onConfirm={handleCleanupDanglingFks}
                onCancel={() => setCleanupConfirmOpen(false)}
            />
            {leaveDialog}
        </>
    );
};

export default React.memo(DataFolderConfigForm);
