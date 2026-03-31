import React, { useCallback, useEffect, useMemo, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AlertTriangle, CheckCircle2, Cloud, Database, HardDriveDownload, RefreshCw } from "lucide-react";
import ConfigForm from "@/components/ConfigForm";
import { Button } from "@/components/ui/button";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";

type SyncMode = "Manual" | "Auto";
type FirstSyncStrategy =
    | "UseRemote"
    | "UseLocal"
    | "AppendLocal"
    | "BackupThenUseRemote";
type SyncStatus = "Never" | "Success" | "Error";

interface SyncConfig {
    enabled: boolean;
    server_url: string | null;
    auth_token: string | null;
    sync_mode: SyncMode;
    sync_interval_secs: number;
    first_sync_strategy: FirstSyncStrategy;
    initial_sync_completed: boolean;
    last_sync_started_at: string | null;
    last_sync_finished_at: string | null;
    last_sync_status: SyncStatus;
    last_sync_message: string | null;
}

interface DataFolderConfigFormProps {
    form: UseFormReturn<any>;
}

const DEFAULT_SYNC_CONFIG: SyncConfig = {
    enabled: false,
    server_url: "",
    auth_token: "",
    sync_mode: "Auto",
    sync_interval_secs: 60,
    first_sync_strategy: "UseRemote",
    initial_sync_completed: false,
    last_sync_started_at: null,
    last_sync_finished_at: null,
    last_sync_status: "Never",
    last_sync_message: null,
};

const strategyDescriptionMap: Record<FirstSyncStrategy, string> = {
    UseRemote: "使用云端数据（推荐）—— 丢弃本设备当前数据，以云端为准。",
    UseLocal: "上传本设备数据 —— 以本地数据覆盖云端已有数据。",
    AppendLocal: "合并数据（Append，暂未支持）—— 直连 sqld 模式下暂不提供自动合并与去重。",
    BackupThenUseRemote: "先备份再使用云端 —— 建议先打开数据目录备份，再让云端覆盖本地。",
};

const statusVariantMap: Record<SyncStatus, "outline" | "default" | "destructive"> = {
    Never: "outline",
    Success: "default",
    Error: "destructive",
};

const statusLabelMap: Record<SyncStatus, string> = {
    Never: "未同步",
    Success: "最近一次成功",
    Error: "最近一次失败",
};

function formatDateTime(value: string | null | undefined): string {
    if (!value) {
        return "—";
    }

    const date = new Date(value);
    if (Number.isNaN(date.getTime())) {
        return value;
    }
    return date.toLocaleString("zh-CN");
}

export const DataFolderConfigForm: React.FC<DataFolderConfigFormProps> = ({ form }) => {
    const [loading, setLoading] = useState(true);
    const [syncing, setSyncing] = useState(false);
    const [syncConfig, setSyncConfig] = useState<SyncConfig>(DEFAULT_SYNC_CONFIG);

    const enabled = Boolean(form.watch("enabled"));
    const syncMode = (form.watch("sync_mode") as SyncMode | undefined) || "Auto";
    const firstSyncStrategy =
        (form.watch("first_sync_strategy") as FirstSyncStrategy | undefined) || "UseRemote";

    const loadSyncConfig = useCallback(async () => {
        setLoading(true);
        try {
            const config = await invoke<SyncConfig>("get_sync_config");
            setSyncConfig(config);
            form.reset({
                enabled: config.enabled,
                server_url: config.server_url || "",
                auth_token: config.auth_token || "",
                sync_mode: config.sync_mode || "Auto",
                sync_interval_secs: String(config.sync_interval_secs || 60),
                first_sync_strategy: config.first_sync_strategy || "UseRemote",
            });
        } catch (error) {
            toast.error("加载同步配置失败: " + getErrorMessage(error));
            setSyncConfig(DEFAULT_SYNC_CONFIG);
            form.reset({
                enabled: DEFAULT_SYNC_CONFIG.enabled,
                server_url: DEFAULT_SYNC_CONFIG.server_url,
                auth_token: DEFAULT_SYNC_CONFIG.auth_token,
                sync_mode: DEFAULT_SYNC_CONFIG.sync_mode,
                sync_interval_secs: String(DEFAULT_SYNC_CONFIG.sync_interval_secs),
                first_sync_strategy: DEFAULT_SYNC_CONFIG.first_sync_strategy,
            });
        } finally {
            setLoading(false);
        }
    }, [form]);

    useEffect(() => {
        void loadSyncConfig();
    }, [loadSyncConfig]);

    useEffect(() => {
        let active = true;

        const setupListeners = async () => {
            const unlistenStatus = await listen<SyncConfig>("sync_status_changed", (event) => {
                if (!active) {
                    return;
                }
                setSyncConfig(event.payload);
            });

            const unlistenConfig = await listen<SyncConfig>("sync_config_changed", (event) => {
                if (!active) {
                    return;
                }
                setSyncConfig(event.payload);
                form.reset({
                    enabled: event.payload.enabled,
                    server_url: event.payload.server_url || "",
                    auth_token: event.payload.auth_token || "",
                    sync_mode: event.payload.sync_mode || "Auto",
                    sync_interval_secs: String(event.payload.sync_interval_secs || 60),
                    first_sync_strategy: event.payload.first_sync_strategy || "UseRemote",
                });
            });

            const unlistenCompleted = await listen<string>("sync_run_completed", (event) => {
                if (!active) {
                    return;
                }
                setSyncing(false);
                if (event.payload) {
                    toast.success(event.payload);
                }
                void loadSyncConfig();
            });

            return () => {
                unlistenStatus();
                unlistenConfig();
                unlistenCompleted();
            };
        };

        const cleanupPromise = setupListeners();

        return () => {
            active = false;
            cleanupPromise.then((cleanup) => cleanup());
        };
    }, [form, loadSyncConfig]);

    const handleOpenDataFolder = useCallback(async () => {
        try {
            await invoke("open_data_folder");
        } catch (error) {
            toast.error("打开数据目录失败: " + getErrorMessage(error));
        }
    }, []);

    const handleRunSyncNow = useCallback(async () => {
        const values = form.getValues();
        const selectedStrategy =
            (values.first_sync_strategy as FirstSyncStrategy | undefined) || syncConfig.first_sync_strategy;
        if (!syncConfig.initial_sync_completed && selectedStrategy === "AppendLocal") {
            toast.error("当前直连 sqld 同步暂不支持 AppendLocal，请改用“使用云端数据”或“上传本设备数据”。");
            return;
        }
        setSyncing(true);
        try {
            const message = await invoke<string>("run_sync_now");
            toast.success(message || "同步完成");
            await loadSyncConfig();
        } catch (error) {
            toast.error("执行同步失败: " + getErrorMessage(error));
            await loadSyncConfig();
        } finally {
            setSyncing(false);
        }
    }, [loadSyncConfig]);

    const handleResetOnboarding = useCallback(async () => {
        try {
            await invoke("reset_sync_onboarding");
            toast.success("已重置首次同步向导");
            await loadSyncConfig();
        } catch (error) {
            toast.error("重置首次同步向导失败: " + getErrorMessage(error));
        }
    }, [loadSyncConfig]);

    const handleSave = useCallback(async () => {
        try {
            const values = form.getValues();
            const payload: SyncConfig = {
                ...syncConfig,
                enabled: Boolean(values.enabled),
                server_url: values.server_url?.trim() ? values.server_url.trim() : null,
                auth_token: values.auth_token?.trim() ? values.auth_token.trim() : null,
                sync_mode: (values.sync_mode as SyncMode) || "Auto",
                sync_interval_secs: Number(values.sync_interval_secs || 60),
                first_sync_strategy:
                    (values.first_sync_strategy as FirstSyncStrategy) || "UseRemote",
            };
            if (payload.enabled && !payload.initial_sync_completed && payload.first_sync_strategy === "AppendLocal") {
                toast.error("当前直连 sqld 同步暂不支持 AppendLocal，请改用“使用云端数据”或“上传本设备数据”。");
                return;
            }
            await invoke("save_sync_config", { config: payload });
            toast.success(payload.enabled ? "同步配置已保存" : "已切换为纯本地模式");
            await loadSyncConfig();
        } catch (error) {
            toast.error("保存同步配置失败: " + getErrorMessage(error));
        }
    }, [form, loadSyncConfig, syncConfig]);

    const syncOverview = useMemo(() => {
        if (loading) {
            return "正在读取同步配置...";
        }
        if (!syncConfig.enabled) {
            return "当前为纯本地模式，不会进行任何联网同步。";
        }
        const modeLabel = syncConfig.sync_mode === "Auto" ? "自动同步" : "手动同步";
        const firstSyncLabel = syncConfig.initial_sync_completed ? "已完成首次同步" : "尚未完成首次同步";
        return `${modeLabel} · ${firstSyncLabel} · ${strategyDescriptionMap[syncConfig.first_sync_strategy]}`;
    }, [loading, syncConfig]);

    const syncMessage =
        syncConfig.last_sync_message ||
        (syncConfig.enabled
            ? "保存配置后即可开始同步。"
            : "关闭同步后，本地读写性能与以前保持一致。");

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
                description: "关闭时完全使用本地数据库，不产生额外网络和同步开销。",
            },
        },
        {
            key: "sync_mode",
            config: {
                type: "radio" as const,
                label: "同步方式",
                hidden: !enabled,
                disabled: loading || !enabled,
                options: [
                    { value: "Auto", label: "自动同步（推荐）", tooltip: "首次同步完成后，后台按固定间隔执行本地/云端同步" },
                    { value: "Manual", label: "手动同步", tooltip: "平时只写本地，只有点击“立即同步”时才与云端交换数据" },
                ],
            },
        },
        {
            key: "server_url",
            config: {
                type: "input" as const,
                label: "同步服务地址",
                placeholder: "http://127.0.0.1:8080",
                hidden: !enabled,
                disabled: loading || !enabled,
                description: "填写自建 sqld / libSQL 根地址；AIPP 会按数据库自动路由到 /dev/<namespace>。",
            },
        },
        {
            key: "auth_token",
            config: {
                type: "password" as const,
                label: "访问令牌",
                placeholder: "输入同步服务 JWT / Token",
                hidden: !enabled,
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
                hidden: !enabled || syncMode !== "Auto",
                disabled: loading || !enabled || syncMode !== "Auto",
                description: "仅在自动同步模式下生效。",
            },
        },
        {
            key: "first_sync_strategy",
            config: {
                type: "radio" as const,
                label: "首次同步策略",
                hidden: !enabled || syncConfig.initial_sync_completed,
                disabled: loading || !enabled,
                options: [
                    { value: "UseRemote", label: "📥 使用云端数据（推荐）" },
                    { value: "UseLocal", label: "📤 上传本设备数据" },
                    { value: "AppendLocal", label: "🔀 合并数据（Append，暂未支持）" },
                    { value: "BackupThenUseRemote", label: "📋 先备份本地数据再同步" },
                ],
            },
        },
        {
            key: "sync_status",
            config: {
                type: "inline-buttons" as const,
                label: "同步操作",
                value: syncMessage,
                buttons: [
                    {
                        text: syncing ? "同步中..." : "立即同步",
                        onClick: handleRunSyncNow,
                        disabled: loading || syncing || !enabled || (!syncConfig.initial_sync_completed && firstSyncStrategy === "AppendLocal"),
                    },
                    {
                        text: "重置首次同步",
                        onClick: handleResetOnboarding,
                        disabled: loading || !enabled,
                        variant: "ghost" as const,
                    },
                ],
            },
        },
    ];

    return (
        <div className="space-y-6">
            <Card>
                <CardHeader className="pb-3">
                    <div className="flex items-start justify-between gap-3">
                        <div>
                            <CardTitle className="flex items-center gap-2">
                                <Cloud className="h-5 w-5" />
                                同步概览
                            </CardTitle>
                            <CardDescription className="mt-1">
                                这里负责决定本设备是纯本地运行，还是加入云端同步。
                            </CardDescription>
                        </div>
                        <Badge variant={statusVariantMap[syncConfig.last_sync_status]}>
                            {statusLabelMap[syncConfig.last_sync_status]}
                        </Badge>
                    </div>
                </CardHeader>
                <CardContent className="space-y-4 text-sm">
                    <div className="grid gap-3 md:grid-cols-2">
                        <div className="rounded-lg border bg-muted/30 p-3">
                            <div className="font-medium">当前模式</div>
                            <div className="mt-1 text-muted-foreground">
                                {syncConfig.enabled
                                    ? syncConfig.sync_mode === "Auto"
                                        ? "已启用自动同步"
                                        : "已启用手动同步"
                                    : "纯本地模式"}
                            </div>
                        </div>
                        <div className="rounded-lg border bg-muted/30 p-3">
                            <div className="font-medium">首次同步</div>
                            <div className="mt-1 text-muted-foreground">
                                {syncConfig.initial_sync_completed ? "已完成" : "尚未完成"}
                            </div>
                        </div>
                    </div>

                    <div className="rounded-lg border bg-muted/20 p-3">
                        <div className="font-medium">当前说明</div>
                        <div className="mt-1 text-muted-foreground">{syncOverview}</div>
                    </div>

                    <Separator />

                    <div className="grid gap-2 text-muted-foreground">
                        <div>上次开始：{formatDateTime(syncConfig.last_sync_started_at)}</div>
                        <div>上次完成：{formatDateTime(syncConfig.last_sync_finished_at)}</div>
                        <div>最近结果：{syncMessage}</div>
                    </div>
                </CardContent>
            </Card>

            {!syncConfig.initial_sync_completed && enabled && (
                <Alert>
                    <HardDriveDownload className="h-4 w-4" />
                    <AlertTitle>首次开启同步时，请先选好策略</AlertTitle>
                    <AlertDescription>
                        <p>{strategyDescriptionMap[firstSyncStrategy]}</p>
                        <p>如果多台设备之前已经各自积累了不同本地数据，libSQL/sqld 不会自动做 CRDT 式冲突合并。</p>
                    </AlertDescription>
                </Alert>
            )}

            {enabled && firstSyncStrategy === "AppendLocal" && !syncConfig.initial_sync_completed && (
                <Alert variant="destructive">
                    <AlertTriangle className="h-4 w-4" />
                    <AlertTitle>AppendLocal 当前不可用</AlertTitle>
                    <AlertDescription>
                        <p>当前版本的直连 sqld 同步不提供“本地 + 云端”自动合并、去重和 ID 重映射。</p>
                        <p>请改用“使用云端数据”或“上传本设备数据”，避免造成重复记录或不一致状态。</p>
                    </AlertDescription>
                </Alert>
            )}

            {enabled && firstSyncStrategy === "BackupThenUseRemote" && !syncConfig.initial_sync_completed && (
                <Alert>
                    <CheckCircle2 className="h-4 w-4" />
                    <AlertTitle>建议先备份本地目录</AlertTitle>
                    <AlertDescription>
                        <p>你可以先打开上面的数据文件夹，手动备份当前本地数据库，再执行首次同步。</p>
                    </AlertDescription>
                </Alert>
            )}

            <ConfigForm
                title="数据目录"
                description="管理本地数据目录，并配置完整的多端同步行为与首次同步策略。"
                config={dataFolderConfig}
                layout="default"
                classNames="bottom-space"
                useFormReturn={form}
                onSave={handleSave}
                extraButtons={
                    <div className="flex items-center gap-2">
                        <Button
                            type="button"
                            variant="outline"
                            onClick={() => void loadSyncConfig()}
                            disabled={loading}
                        >
                            <RefreshCw className="mr-2 h-4 w-4" />
                            刷新状态
                        </Button>
                        <Button
                            type="button"
                            variant="outline"
                            onClick={handleOpenDataFolder}
                        >
                            <Database className="mr-2 h-4 w-4" />
                            打开数据目录
                        </Button>
                    </div>
                }
            />
        </div>
    );
};

export default React.memo(DataFolderConfigForm);
