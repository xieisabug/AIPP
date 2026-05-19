import React, { useCallback, useEffect, useMemo, useState } from "react";
import { UseFormReturn, useWatch } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import ConfigForm from "@/components/ConfigForm";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

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
    last_sync_at?: string | null;
    last_error?: string | null;
    pending_outbox_count: number;
    pushing_outbox_count: number;
    failed_outbox_count: number;
    server_cursor: number;
}

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({ form }) => {
    const [status, setStatus] = useState<SyncStatus | null>(null);
    const [saving, setSaving] = useState(false);
    const [syncing, setSyncing] = useState(false);
    const watchedMode = useWatch({ control: form.control, name: "mode" });
    const mode = watchedMode || "local";

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
            toast.error("读取同步状态失败: " + getErrorMessage(error));
        }
    }, [form]);

    useEffect(() => {
        void loadStatus(true);
        const unlistenPromise = listen<SyncStatus>("sync_status_changed", (event) => {
            setStatus(event.payload);
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
        }, 1500);
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

    const handleSave = useCallback(async () => {
        const values = form.getValues();
        setSaving(true);
        try {
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
            toast.success(nextStatus.mode === "self_hosted" ? "自建同步配置已保存" : "已切换为本地模式");
            if (nextStatus.mode === "self_hosted") {
                void loadStatus(false);
            }
        } catch (error) {
            toast.error("保存同步配置失败: " + getErrorMessage(error));
        } finally {
            setSaving(false);
        }
    }, [form]);

    const handleSyncNow = useCallback(async () => {
        setSyncing(true);
        try {
            const nextStatus = await invoke<SyncStatus>("trigger_sync_now");
            setStatus(nextStatus);
            toast.success("同步已触发");
        } catch (error) {
            toast.error("触发同步失败: " + getErrorMessage(error));
        } finally {
            setSyncing(false);
        }
    }, []);

    const statusText = useMemo(() => {
        if (!status) {
            return "读取中";
        }
        if (status.mode === "local") {
            return "本地模式：仅使用本机 SQLite，后台同步已停止。";
        }
        const state = status.syncing ? "同步中" : status.connected ? "已连接" : status.running ? "等待同步" : "未运行";
        return `${state}；待推送 ${status.pending_outbox_count}，推送中 ${status.pushing_outbox_count ?? 0}，失败 ${status.failed_outbox_count}，游标 ${status.server_cursor}`;
    }, [status]);

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
                        tooltip: "连接你自建的 AIPP sync-server，按业务对象同步核心数据。",
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
            key: "status",
            config: {
                type: "static" as const,
                label: "连接状态",
                value: statusText,
            },
        },
        {
            key: "lastSyncAt",
            config: {
                type: "static" as const,
                label: "上次同步时间",
                value: status?.last_sync_at || "尚未同步",
            },
        },
        {
            key: "lastError",
            config: {
                type: "static" as const,
                label: "最近错误",
                value: status?.last_error || "无",
            },
        },
        {
            key: "syncNow",
            config: {
                type: "button" as const,
                label: "手动同步",
                value: syncing ? "同步中..." : "立即同步",
                disabled: mode !== "self_hosted" || syncing || saving,
                onClick: handleSyncNow,
            },
        },
    ];

    return (
        <ConfigForm
            title="数据目录"
            description="管理本地数据目录与自建同步。"
            config={dataFolderConfig}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
            onSave={handleSave}
        />
    );
};

export default React.memo(DataFolderConfigForm);
