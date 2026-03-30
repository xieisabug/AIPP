import React, { useCallback, useEffect, useMemo, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import ConfigForm from "@/components/ConfigForm";
import { Button } from "@/components/ui/button";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

interface SyncConfig {
    enabled: boolean;
    server_url: string | null;
    auth_token: string | null;
    sync_interval_secs: number;
}

interface DataFolderConfigFormProps {
    form: UseFormReturn<any>;
}

const DEFAULT_SYNC_CONFIG: SyncConfig = {
    enabled: false,
    server_url: "",
    auth_token: "",
    sync_interval_secs: 60,
};

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({ form }) => {
    const [loading, setLoading] = useState(true);
    const [syncing, setSyncing] = useState(false);

    const enabled = Boolean(form.watch("enabled"));
    const serverUrl = form.watch("server_url") || "";
    const interval = form.watch("sync_interval_secs") || "60";

    const loadSyncConfig = useCallback(async () => {
        setLoading(true);
        try {
            const config = await invoke<SyncConfig>("get_sync_config");
            form.reset({
                enabled: config.enabled,
                server_url: config.server_url || "",
                auth_token: config.auth_token || "",
                sync_interval_secs: String(config.sync_interval_secs || 60),
            });
        } catch (error) {
            toast.error("加载同步配置失败: " + getErrorMessage(error));
            form.reset({
                enabled: DEFAULT_SYNC_CONFIG.enabled,
                server_url: DEFAULT_SYNC_CONFIG.server_url,
                auth_token: DEFAULT_SYNC_CONFIG.auth_token,
                sync_interval_secs: String(DEFAULT_SYNC_CONFIG.sync_interval_secs),
            });
        } finally {
            setLoading(false);
        }
    }, [form]);

    useEffect(() => {
        void loadSyncConfig();
    }, [loadSyncConfig]);

    const handleOpenDataFolder = useCallback(async () => {
        try {
            await invoke("open_data_folder");
        } catch (error) {
            toast.error("打开数据目录失败: " + getErrorMessage(error));
        }
    }, []);

    const handleRunSyncNow = useCallback(async () => {
        setSyncing(true);
        try {
            const message = await invoke<string>("run_sync_now");
            toast.success(message || "同步完成");
        } catch (error) {
            toast.error("执行同步失败: " + getErrorMessage(error));
        } finally {
            setSyncing(false);
        }
    }, []);

    const handleSave = useCallback(async () => {
        try {
            const values = form.getValues();
            const payload: SyncConfig = {
                enabled: Boolean(values.enabled),
                server_url: values.server_url?.trim() ? values.server_url.trim() : null,
                auth_token: values.auth_token?.trim() ? values.auth_token.trim() : null,
                sync_interval_secs: Number(values.sync_interval_secs || 60),
            };
            await invoke("save_sync_config", { config: payload });
            toast.success(payload.enabled ? "同步配置已保存" : "已切换为本地模式");
            await loadSyncConfig();
        } catch (error) {
            toast.error("保存同步配置失败: " + getErrorMessage(error));
        }
    }, [form, loadSyncConfig]);

    const syncStatus = useMemo(() => {
        if (loading) {
            return "正在读取同步配置...";
        }
        if (!enabled) {
            return "当前为纯本地模式，不会与远端同步。";
        }
        return `已启用多端同步，目标服务：${serverUrl || "未填写"}，间隔：${interval} 秒。`;
    }, [enabled, interval, loading, serverUrl]);

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
            key: "enabled",
            config: {
                type: "switch" as const,
                label: "启用多端同步",
                disabled: loading,
                description: "关闭时完全使用本地数据库，不产生额外同步开销。",
            },
        },
        {
            key: "server_url",
            config: {
                type: "input" as const,
                label: "同步服务地址",
                placeholder: "http://127.0.0.1:8080",
                disabled: loading || !enabled,
                description: "填写自建 sqld / libSQL 服务地址。",
            },
        },
        {
            key: "auth_token",
            config: {
                type: "password" as const,
                label: "访问令牌",
                placeholder: "输入同步服务 JWT / Token",
                disabled: loading || !enabled,
                description: "用于访问同步服务的认证令牌。",
            },
        },
        {
            key: "sync_interval_secs",
            config: {
                type: "input" as const,
                label: "自动同步间隔（秒）",
                placeholder: "60",
                disabled: loading || !enabled,
                description: "后台会按此间隔自动执行同步。",
            },
        },
        {
            key: "sync_status",
            config: {
                type: "inline-buttons" as const,
                label: "同步状态",
                value: syncStatus,
                buttons: [
                    {
                        text: syncing ? "同步中..." : "立即同步",
                        onClick: handleRunSyncNow,
                        disabled: loading || syncing || !enabled,
                    },
                ],
            },
        },
    ];

    return (
        <ConfigForm
            title="数据目录"
            description="管理本地数据目录，并配置可选的多端同步。"
            config={dataFolderConfig}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
            onSave={handleSave}
            extraButtons={
                <Button
                    type="button"
                    variant="outline"
                    onClick={() => void loadSyncConfig()}
                    disabled={loading}
                >
                    刷新状态
                </Button>
            }
        />
    );
};

export default React.memo(DataFolderConfigForm);
