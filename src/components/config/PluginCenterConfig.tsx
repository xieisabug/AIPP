import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Puzzle, RefreshCcw, Power, PowerOff, Trash2 } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import {
    AlertDialog,
    AlertDialogAction,
    AlertDialogCancel,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "../ui/alert-dialog";
import { ConfigPageLayout, EmptyState, ListItemButton, SidebarList, type SelectOption } from "../common";
import { pluginRuntime } from "../../services/PluginRuntime";
import PluginViewHost from "../plugin/PluginViewHost";
import type { LoadedPlugin } from "../../services/PluginRuntime";

interface PluginCenterConfigProps {
    pluginList: LoadedPlugin[];
}

interface PluginRegistryItem {
    pluginId: number;
    name: string;
    version: string;
    code: string;
    description?: string | null;
    author?: string | null;
    pluginType: string[];
    permissions?: string[];
    runtime?: {
        type: string;
        entry: string;
        protocol?: string | null;
        checksum?: string | null;
    } | null;
    contributions?: {
        hooks?: Array<{
            name: string;
            kind?: string;
            priority?: number;
            timeoutMs?: number;
            failurePolicy?: string;
            isActive?: boolean;
        }>;
        views?: Array<{
            id: string;
            location: string;
            title: string;
            description?: string | null;
        }>;
        assistantFormFields?: Array<{
            key: string;
            label: string;
            type: string;
        }>;
    };
    isActive: boolean;
    isInstalled: boolean;
}

interface PluginConfigItem {
    configId: number;
    pluginId: number;
    configKey: string;
    configValue?: string | null;
}

interface PluginHookRegistrationItem {
    id: number;
    pluginId: number;
    hookName: string;
    hookKind: string;
    priority: number;
    timeoutMs: number;
    failurePolicy: string;
    isActive: boolean;
}

interface PluginHookAuditLogItem {
    id: number;
    pluginId: number;
    hookName: string;
    conversationId?: number | null;
    messageId?: number | null;
    status: string;
    action?: string | null;
    durationMs?: number | null;
    error?: string | null;
    createdAt: string;
}

const PLUGIN_TYPE_LABELS: Record<string, string> = {
    interfaceType: "界面",
    applicationType: "应用",
    toolType: "工具",
    themeType: "主题",
    markdownType: "Markdown",
    messageType: "消息",
    exportType: "导出",
};

const getPluginTypeLabel = (type: string) => PLUGIN_TYPE_LABELS[type] ?? type;

const PluginCenterConfig: React.FC<PluginCenterConfigProps> = ({ pluginList }) => {
    const [plugins, setPlugins] = useState<PluginRegistryItem[]>([]);
    const [runtimePlugins, setRuntimePlugins] = useState<LoadedPlugin[]>(pluginList);
    const [runtimeRefreshing, setRuntimeRefreshing] = useState(false);
    const [selectedPluginId, setSelectedPluginId] = useState<number | null>(null);
    const [searchQuery, setSearchQuery] = useState("");
    const [loading, setLoading] = useState(false);
    const [configs, setConfigs] = useState<PluginConfigItem[]>([]);
    const [configLoading, setConfigLoading] = useState(false);
    const [newConfigKey, setNewConfigKey] = useState("");
    const [newConfigValue, setNewConfigValue] = useState("");
    const [hookRegistrations, setHookRegistrations] = useState<PluginHookRegistrationItem[]>([]);
    const [hookAuditLogs, setHookAuditLogs] = useState<PluginHookAuditLogItem[]>([]);
    const [hookDebugLoading, setHookDebugLoading] = useState(false);
    const [actionBusy, setActionBusy] = useState(false);
    const [pendingUninstallPlugin, setPendingUninstallPlugin] = useState<PluginRegistryItem | null>(null);
    const missingReminderKeyRef = useRef("");

    useEffect(() => {
        setRuntimePlugins(pluginList);
    }, [pluginList]);

    const loadedPluginByCode = useMemo(() => {
        const map = new Map<string, any>();
        runtimePlugins.forEach((plugin) => {
            map.set(plugin.code, plugin);
        });
        return map;
    }, [runtimePlugins]);

    const syncRuntimePlugins = useCallback(async (forceReload = true) => {
        setRuntimeRefreshing(true);
        try {
            const items = forceReload
                ? await pluginRuntime.reloadPlugins()
                : await pluginRuntime.loadPlugins();
            setRuntimePlugins(items);
            return items;
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to refresh runtime plugins:", error);
            setRuntimePlugins([]);
            throw error;
        } finally {
            setRuntimeRefreshing(false);
        }
    }, []);

    const loadPlugins = useCallback(async () => {
        setLoading(true);
        try {
            const items = await invoke<PluginRegistryItem[]>("list_plugins");
            setPlugins(items);
            const missingPlugins = items.filter((item) => !item.isInstalled);
            const reminderKey = missingPlugins
                .map((item) => item.code)
                .sort()
                .join(",");
            if (missingPlugins.length > 0 && reminderKey !== missingReminderKeyRef.current) {
                const preview = missingPlugins
                    .map((item) => item.name)
                    .slice(0, 3)
                    .join("、");
                const suffix = missingPlugins.length > 3 ? ` 等${missingPlugins.length}个` : "";
                toast.warning(`发现插件目录缺失，请在插件中心手动卸载：${preview}${suffix}`);
            }
            missingReminderKeyRef.current = reminderKey;
            setSelectedPluginId((prev) => {
                if (prev && items.some((item) => item.pluginId === prev)) {
                    return prev;
                }
                return items.length > 0 ? items[0].pluginId : null;
            });
            await syncRuntimePlugins(true);
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to load plugins:", error);
            toast.error("加载插件列表失败");
        } finally {
            setLoading(false);
        }
    }, [syncRuntimePlugins]);

    const selectedPlugin = useMemo(
        () => plugins.find((item) => item.pluginId === selectedPluginId) || null,
        [plugins, selectedPluginId]
    );

    const loadConfigs = useCallback(async (pluginId: number) => {
        setConfigLoading(true);
        try {
            const result = await invoke<PluginConfigItem[]>("get_plugin_config", { pluginId });
            setConfigs(result);
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to load plugin configs:", error);
            toast.error("加载插件配置失败");
        } finally {
            setConfigLoading(false);
        }
    }, []);

    const loadHookDebugInfo = useCallback(async (pluginId: number) => {
        setHookDebugLoading(true);
        try {
            const [registrations, logs] = await Promise.all([
                invoke<PluginHookRegistrationItem[]>("get_plugin_hook_registrations", { pluginId }),
                invoke<PluginHookAuditLogItem[]>("list_plugin_hook_audit_logs", { limit: 100 }),
            ]);
            setHookRegistrations(registrations);
            setHookAuditLogs(logs.filter((item) => item.pluginId === pluginId));
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to load hook debug info:", error);
            toast.error("加载插件 Hook 调试信息失败");
        } finally {
            setHookDebugLoading(false);
        }
    }, []);

    useEffect(() => {
        loadPlugins();
        const unlistenRegistryChanged = listen("plugin_registry_changed", () => {
            loadPlugins();
        });
        return () => {
            unlistenRegistryChanged.then((unlisten) => unlisten());
        };
    }, [loadPlugins]);

    useEffect(() => {
        if (selectedPluginId) {
            loadConfigs(selectedPluginId);
            loadHookDebugInfo(selectedPluginId);
        } else {
            setConfigs([]);
            setHookRegistrations([]);
            setHookAuditLogs([]);
        }
    }, [selectedPluginId, loadConfigs, loadHookDebugInfo]);

    const filteredPlugins = useMemo(() => {
        const query = searchQuery.trim().toLowerCase();
        if (!query) {
            return plugins;
        }
        return plugins.filter(
            (plugin) =>
                plugin.name.toLowerCase().includes(query) ||
                plugin.code.toLowerCase().includes(query) ||
                (plugin.description || "").toLowerCase().includes(query)
        );
    }, [plugins, searchQuery]);

    const selectOptions = useMemo<SelectOption[]>(
        () =>
            filteredPlugins.map((plugin) => ({
                id: String(plugin.pluginId),
                label: plugin.isInstalled ? plugin.name : `${plugin.name}（目录缺失）`,
                icon: !plugin.isInstalled
                    ? <PowerOff className="h-4 w-4 text-destructive" />
                    : plugin.isActive
                        ? <Power className="h-4 w-4 text-emerald-500" />
                        : <PowerOff className="h-4 w-4 text-muted-foreground" />,
            })),
        [filteredPlugins]
    );

    const currentLoadedPlugin = selectedPlugin ? loadedPluginByCode.get(selectedPlugin.code) : null;
    const selectedPluginViewHostItems = useMemo<LoadedPlugin[]>(() => {
        if (!selectedPlugin) {
            return [];
        }
        if (currentLoadedPlugin) {
            return [currentLoadedPlugin];
        }
        return [
            {
                pluginId: selectedPlugin.pluginId,
                name: selectedPlugin.name,
                version: selectedPlugin.version,
                code: selectedPlugin.code,
                pluginType: selectedPlugin.pluginType,
                contributions: selectedPlugin.contributions,
                instance: null,
            },
        ];
    }, [selectedPlugin, currentLoadedPlugin]);
    const hasInterfacePluginType = selectedPlugin?.pluginType.includes("interfaceType") ?? false;
    const selectedRuntimeType = selectedPlugin?.runtime?.type ?? "js";
    const pluginUiBlockedReason = useMemo(() => {
        if (!selectedPlugin) {
            return "请选择一个插件。";
        }
        if (!selectedPlugin.isInstalled) {
            return "插件目录缺失，请先卸载该记录。";
        }
        if (!selectedPlugin.isActive) {
            return "插件已禁用，启用后可使用插件界面。";
        }
        if (!hasInterfacePluginType) {
            return "该插件未提供界面能力。";
        }
        if (selectedRuntimeType !== "js") {
            return "该插件不是 JS 运行时，不会加载前端插件界面。";
        }
        if (!currentLoadedPlugin) {
            return "插件运行时未加载该插件，请刷新插件运行时。";
        }
        if (!currentLoadedPlugin.instance) {
            return "插件加载失败（实例为空），请检查插件脚本导出。";
        }
        if (typeof currentLoadedPlugin.instance?.renderComponent !== "function") {
            return "插件已加载，但未实现 renderComponent()。";
        }
        return null;
    }, [selectedPlugin, currentLoadedPlugin, hasInterfacePluginType, selectedRuntimeType]);
    const canRenderPluginUI = !pluginUiBlockedReason;

    const handleTogglePlugin = useCallback(async () => {
        if (!selectedPlugin || actionBusy) {
            return;
        }
        setActionBusy(true);
        try {
            if (selectedPlugin.isActive) {
                await invoke("disable_plugin", { pluginId: selectedPlugin.pluginId });
                toast.success(`已禁用插件：${selectedPlugin.name}`);
            } else {
                await invoke("enable_plugin", { pluginId: selectedPlugin.pluginId });
                toast.success(`已启用插件：${selectedPlugin.name}`);
            }
            await loadPlugins();
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to toggle plugin status:", error);
            toast.error("插件启停失败");
        } finally {
            setActionBusy(false);
        }
    }, [selectedPlugin, actionBusy, loadPlugins]);

    const handleSaveConfig = useCallback(async () => {
        if (!selectedPlugin || !newConfigKey.trim()) {
            toast.error("请填写配置键");
            return;
        }
        setActionBusy(true);
        try {
            await invoke("set_plugin_config", {
                pluginId: selectedPlugin.pluginId,
                key: newConfigKey.trim(),
                value: newConfigValue.trim() ? newConfigValue : null,
            });
            toast.success("插件配置已保存");
            setNewConfigKey("");
            setNewConfigValue("");
            await loadConfigs(selectedPlugin.pluginId);
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to save plugin config:", error);
            toast.error("保存插件配置失败");
        } finally {
            setActionBusy(false);
        }
    }, [selectedPlugin, newConfigKey, newConfigValue, loadConfigs]);

    const handleRequestUninstallPlugin = useCallback(() => {
        if (!selectedPlugin || actionBusy) {
            return;
        }
        setPendingUninstallPlugin(selectedPlugin);
    }, [selectedPlugin, actionBusy]);

    const handleConfirmUninstallPlugin = useCallback(async () => {
        const plugin = pendingUninstallPlugin;
        if (!plugin || actionBusy) {
            return;
        }
        setActionBusy(true);
        try {
            await invoke("uninstall_plugin", { pluginId: plugin.pluginId });
            toast.success(`已卸载插件：${plugin.name}`);
            setPendingUninstallPlugin(null);
            await loadPlugins();
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to uninstall plugin:", error);
            toast.error("卸载插件失败");
        } finally {
            setActionBusy(false);
        }
    }, [pendingUninstallPlugin, actionBusy, loadPlugins]);

    const pluginUiNode = useMemo(() => {
        if (!canRenderPluginUI) {
            return null;
        }
        try {
            return currentLoadedPlugin.instance.renderComponent?.() ?? null;
        } catch (error) {
            console.error("[PluginCenterConfig] Plugin renderComponent failed:", error);
            return (
                <div className="text-sm text-destructive">
                    插件界面渲染失败，请检查插件实现。
                </div>
            );
        }
    }, [canRenderPluginUI, currentLoadedPlugin]);

    const sidebar = (
        <SidebarList
            title="插件中心"
            description="统一管理已安装插件及其配置"
            icon={<Puzzle className="h-5 w-5" />}
            searchValue={searchQuery}
            onSearchChange={setSearchQuery}
            searchPlaceholder="搜索插件..."
            addButton={
                <Button variant="outline" size="icon" onClick={loadPlugins} disabled={loading}>
                    <RefreshCcw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
                </Button>
            }
        >
            {filteredPlugins.map((plugin) => (
                <ListItemButton
                    key={plugin.pluginId}
                    isSelected={selectedPluginId === plugin.pluginId}
                    onClick={() => setSelectedPluginId(plugin.pluginId)}
                    className="h-auto py-2.5"
                >
                    <div className="flex flex-col items-start gap-1">
                        <span className="font-medium">{plugin.name}</span>
                        {!plugin.isInstalled && (
                            <span className="text-xs text-destructive">目录缺失，请卸载</span>
                        )}
                    </div>
                </ListItemButton>
            ))}
        </SidebarList>
    );

    const content = !selectedPlugin ? (
        <EmptyState
            icon={<Puzzle className="h-8 w-8 text-muted-foreground" />}
            title="暂无插件"
            description="请先安装插件，然后在这里进行启停和配置管理。"
        />
    ) : (
        <Card className="shadow-none">
            <CardHeader>
                <div className="flex items-start justify-between gap-3">
                    <div>
                        <CardTitle className="text-lg">{selectedPlugin.name}</CardTitle>
                        <CardDescription className="mt-1">
                            {selectedPlugin.description || "暂无描述"}
                        </CardDescription>
                        <div className="mt-2 flex flex-wrap gap-2">
                            <Badge variant={selectedPlugin.isActive ? "default" : "secondary"}>
                                {selectedPlugin.isActive ? "已启用" : "已禁用"}
                            </Badge>
                            {!selectedPlugin.isInstalled && (
                                <Badge variant="destructive">目录缺失</Badge>
                            )}
                            <Badge variant="outline">v{selectedPlugin.version}</Badge>
                            <Badge variant="outline">Runtime: {selectedRuntimeType.toUpperCase()}</Badge>
                            {(selectedPlugin.contributions?.hooks?.length ?? 0) > 0 && (
                                <Badge variant="outline">{selectedPlugin.contributions?.hooks?.length} Hooks</Badge>
                            )}
                            {(selectedPlugin.contributions?.views?.length ?? 0) > 0 && (
                                <Badge variant="outline">{selectedPlugin.contributions?.views?.length} Views</Badge>
                            )}
                            {(selectedPlugin.contributions?.assistantFormFields?.length ?? 0) > 0 && (
                                <Badge variant="outline">
                                    {selectedPlugin.contributions?.assistantFormFields?.length} Assistant Fields
                                </Badge>
                            )}
                            {selectedPlugin.pluginType.map((type) => (
                                <Badge key={type} variant="outline">
                                    {getPluginTypeLabel(type)}
                                </Badge>
                            ))}
                        </div>
                    </div>
                    <div className="flex items-center gap-2">
                        <Button
                            variant="ghost"
                            size="icon"
                            className="text-muted-foreground"
                            onClick={handleTogglePlugin}
                            disabled={actionBusy || !selectedPlugin.isInstalled}
                            title={selectedPlugin.isActive ? "禁用插件" : "启用插件"}
                            aria-label={selectedPlugin.isActive ? "禁用插件" : "启用插件"}
                        >
                            {selectedPlugin.isActive ? <PowerOff className="h-4 w-4" /> : <Power className="h-4 w-4" />}
                        </Button>
                        <Button
                            variant="ghost"
                            size="icon"
                            className="text-muted-foreground"
                            onClick={handleRequestUninstallPlugin}
                            disabled={actionBusy}
                            title="卸载插件"
                            aria-label="卸载插件"
                        >
                            <Trash2 className="h-4 w-4 text-destructive" />
                        </Button>
                    </div>
                </div>
            </CardHeader>
            <CardContent>
                <Tabs defaultValue="plugin-ui" className="w-full">
                    <TabsList>
                        <TabsTrigger value="plugin-ui">插件界面</TabsTrigger>
                        <TabsTrigger value="views">扩展视图</TabsTrigger>
                        <TabsTrigger value="config">配置KV</TabsTrigger>
                        <TabsTrigger value="hooks">Hook 调试</TabsTrigger>
                    </TabsList>
                    <TabsContent value="plugin-ui" className="mt-4 space-y-3">
                        <div className="rounded-lg border border-border/60 bg-background p-3 md:p-4">
                            {canRenderPluginUI ? (
                                pluginUiNode
                            ) : (
                                <div className="text-sm text-muted-foreground">
                                    {pluginUiBlockedReason}
                                </div>
                            )}
                        </div>
                        {selectedPlugin.isInstalled &&
                            selectedPlugin.isActive &&
                            hasInterfacePluginType && (
                                <Button
                                    variant="outline"
                                    size="sm"
                                    className="mt-3"
                                    onClick={async () => {
                                        try {
                                            await syncRuntimePlugins(true);
                                            toast.success("插件运行时已刷新");
                                        } catch {
                                            toast.error("刷新插件运行时失败");
                                        }
                                    }}
                                    disabled={runtimeRefreshing}
                                >
                                    <RefreshCcw className={`h-4 w-4 ${runtimeRefreshing ? "animate-spin" : ""}`} />
                                    <span className="ml-2">刷新插件运行时</span>
                                </Button>
                             )}
                     </TabsContent>
                    <TabsContent value="views" className="mt-4 space-y-4">
                        <PluginViewHost
                            pluginList={selectedPluginViewHostItems}
                            location="config.analytics"
                            emptyDescription="当前选中的插件没有在设置页注册扩展视图。"
                        />
                    </TabsContent>
                    <TabsContent value="config" className="mt-4 space-y-4">
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                            <Input
                                placeholder="config key"
                                value={newConfigKey}
                                onChange={(e) => setNewConfigKey(e.target.value)}
                            />
                            <Button onClick={handleSaveConfig} disabled={actionBusy}>
                                保存配置
                            </Button>
                        </div>
                        <Textarea
                            placeholder="config value（可选）"
                            value={newConfigValue}
                            onChange={(e) => setNewConfigValue(e.target.value)}
                            className="min-h-[80px]"
                        />
                        <div className="space-y-2">
                            {configLoading ? (
                                <div className="text-sm text-muted-foreground">加载配置中...</div>
                            ) : configs.length === 0 ? (
                                <div className="text-sm text-muted-foreground">暂无配置项。</div>
                            ) : (
                                configs.map((config) => (
                                    <div
                                        key={config.configId}
                                        className="rounded-md border border-border p-2.5 text-sm"
                                    >
                                        <div className="font-medium">{config.configKey}</div>
                                        <div className="text-muted-foreground break-all">
                                            {config.configValue ?? "(null)"}
                                        </div>
                                    </div>
                                ))
                            )}
                        </div>
                    </TabsContent>
                    <TabsContent value="hooks" className="mt-4 space-y-4">
                        <div className="flex items-center justify-between gap-3">
                            <div>
                                <div className="text-sm font-medium">Hook 注册与执行审计</div>
                                <div className="text-xs text-muted-foreground">
                                    显示当前插件声明的 Hook，以及最近 100 条执行记录中属于该插件的日志。
                                </div>
                            </div>
                            <Button
                                variant="outline"
                                size="sm"
                                onClick={() => selectedPlugin && loadHookDebugInfo(selectedPlugin.pluginId)}
                                disabled={hookDebugLoading}
                            >
                                <RefreshCcw className={`h-4 w-4 ${hookDebugLoading ? "animate-spin" : ""}`} />
                                <span className="ml-2">刷新</span>
                            </Button>
                        </div>
                        <div className="space-y-2">
                            <div className="text-sm font-medium">注册列表</div>
                            {hookDebugLoading ? (
                                <div className="text-sm text-muted-foreground">加载 Hook 信息中...</div>
                            ) : hookRegistrations.length === 0 ? (
                                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                                    当前插件没有注册 Hook。
                                </div>
                            ) : (
                                hookRegistrations.map((hook) => (
                                    <div key={hook.id} className="rounded-md border border-border p-3 text-sm">
                                        <div className="flex flex-wrap items-center gap-2">
                                            <span className="font-medium">{hook.hookName}</span>
                                            <Badge variant={hook.isActive ? "default" : "secondary"}>
                                                {hook.isActive ? "启用" : "禁用"}
                                            </Badge>
                                            <Badge variant="outline">{hook.hookKind}</Badge>
                                            <Badge variant="outline">优先级 {hook.priority}</Badge>
                                        </div>
                                        <div className="mt-1 text-xs text-muted-foreground">
                                            timeout={hook.timeoutMs}ms / failurePolicy={hook.failurePolicy}
                                        </div>
                                    </div>
                                ))
                            )}
                        </div>
                        <div className="space-y-2">
                            <div className="text-sm font-medium">最近执行日志</div>
                            {hookAuditLogs.length === 0 ? (
                                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                                    暂无该插件的 Hook 执行日志。
                                </div>
                            ) : (
                                hookAuditLogs.map((log) => (
                                    <div key={log.id} className="rounded-md border border-border p-3 text-sm">
                                        <div className="flex flex-wrap items-center gap-2">
                                            <span className="font-medium">{log.hookName}</span>
                                            <Badge variant={log.status === "success" ? "default" : "destructive"}>
                                                {log.status}
                                            </Badge>
                                            {log.action && <Badge variant="outline">{log.action}</Badge>}
                                            {typeof log.durationMs === "number" && (
                                                <Badge variant="outline">{log.durationMs}ms</Badge>
                                            )}
                                        </div>
                                        <div className="mt-1 text-xs text-muted-foreground">
                                            {log.createdAt}
                                            {log.conversationId ? ` / conversation ${log.conversationId}` : ""}
                                            {log.messageId ? ` / message ${log.messageId}` : ""}
                                        </div>
                                        {log.error && (
                                            <div className="mt-2 break-all text-xs text-destructive">{log.error}</div>
                                        )}
                                    </div>
                                ))
                            )}
                        </div>
                    </TabsContent>
                </Tabs>
            </CardContent>
        </Card>
    );

    return (
        <>
            <ConfigPageLayout
                sidebar={sidebar}
                content={content}
                showEmptyState={false}
                selectOptions={selectOptions}
                selectedOptionId={selectedPluginId ? String(selectedPluginId) : undefined}
                onSelectOption={(optionId) => setSelectedPluginId(Number(optionId))}
                selectPlaceholder="选择插件"
            />
            <AlertDialog
                open={!!pendingUninstallPlugin}
                onOpenChange={(open) => {
                    if (!open && !actionBusy) {
                        setPendingUninstallPlugin(null);
                    }
                }}
            >
                <AlertDialogContent>
                    <AlertDialogHeader>
                        <AlertDialogTitle>确认卸载插件</AlertDialogTitle>
                        <AlertDialogDescription>
                            {pendingUninstallPlugin
                                ? `卸载后将删除插件「${pendingUninstallPlugin.name}」的文件夹和数据库记录，且不可恢复。`
                                : "卸载后将删除插件文件夹和数据库记录，且不可恢复。"}
                        </AlertDialogDescription>
                    </AlertDialogHeader>
                    <AlertDialogFooter>
                        <AlertDialogCancel disabled={actionBusy}>取消</AlertDialogCancel>
                        <AlertDialogAction
                            className="bg-destructive text-destructive-foreground hover:bg-destructive/90"
                            onClick={handleConfirmUninstallPlugin}
                            disabled={actionBusy}
                        >
                            确认卸载
                        </AlertDialogAction>
                    </AlertDialogFooter>
                </AlertDialogContent>
            </AlertDialog>
        </>
    );
};

export default PluginCenterConfig;
