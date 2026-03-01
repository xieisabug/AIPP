import React, { useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Puzzle, RefreshCcw, Power, PowerOff } from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import { ConfigPageLayout, EmptyState, ListItemButton, SidebarList, type SelectOption } from "../common";
import { pluginRuntime } from "../../services/PluginRuntime";

interface PluginCenterConfigProps {
    pluginList: any[];
}

interface PluginRegistryItem {
    pluginId: number;
    name: string;
    version: string;
    code: string;
    description?: string | null;
    author?: string | null;
    pluginType: string[];
    isActive: boolean;
}

interface PluginConfigItem {
    configId: number;
    pluginId: number;
    configKey: string;
    configValue?: string | null;
}

const PluginCenterConfig: React.FC<PluginCenterConfigProps> = ({ pluginList }) => {
    const [plugins, setPlugins] = useState<PluginRegistryItem[]>([]);
    const [runtimePlugins, setRuntimePlugins] = useState<any[]>(pluginList);
    const [runtimeRefreshing, setRuntimeRefreshing] = useState(false);
    const [selectedPluginId, setSelectedPluginId] = useState<number | null>(null);
    const [searchQuery, setSearchQuery] = useState("");
    const [loading, setLoading] = useState(false);
    const [configs, setConfigs] = useState<PluginConfigItem[]>([]);
    const [configLoading, setConfigLoading] = useState(false);
    const [newConfigKey, setNewConfigKey] = useState("");
    const [newConfigValue, setNewConfigValue] = useState("");
    const [actionBusy, setActionBusy] = useState(false);

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
        } else {
            setConfigs([]);
        }
    }, [selectedPluginId, loadConfigs]);

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
                label: plugin.name,
                icon: plugin.isActive ? <Power className="h-4 w-4 text-emerald-500" /> : <PowerOff className="h-4 w-4 text-muted-foreground" />,
            })),
        [filteredPlugins]
    );

    const currentLoadedPlugin = selectedPlugin ? loadedPluginByCode.get(selectedPlugin.code) : null;
    const pluginUiBlockedReason = useMemo(() => {
        if (!selectedPlugin) {
            return "请选择一个插件。";
        }
        if (!selectedPlugin.isActive) {
            return "插件已禁用，启用后可使用插件界面。";
        }
        if (!selectedPlugin.pluginType.includes("interfaceType")) {
            return "该插件未声明 interfaceType。";
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
    }, [selectedPlugin, currentLoadedPlugin]);
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
                        <span className="text-xs opacity-80">{plugin.code}</span>
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
                            <Badge variant="outline">v{selectedPlugin.version}</Badge>
                            {selectedPlugin.pluginType.map((type) => (
                                <Badge key={type} variant="outline">
                                    {type}
                                </Badge>
                            ))}
                        </div>
                    </div>
                    <Button onClick={handleTogglePlugin} disabled={actionBusy}>
                        {selectedPlugin.isActive ? "禁用插件" : "启用插件"}
                    </Button>
                </div>
            </CardHeader>
            <CardContent>
                <Tabs defaultValue="plugin-ui" className="w-full">
                    <TabsList>
                        <TabsTrigger value="plugin-ui">插件界面</TabsTrigger>
                        <TabsTrigger value="config">配置KV</TabsTrigger>
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
                        {selectedPlugin.isActive &&
                            selectedPlugin.pluginType.includes("interfaceType") && (
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
                </Tabs>
            </CardContent>
        </Card>
    );

    return (
        <ConfigPageLayout
            sidebar={sidebar}
            content={content}
            showEmptyState={false}
            selectOptions={selectOptions}
            selectedOptionId={selectedPluginId ? String(selectedPluginId) : undefined}
            onSelectOption={(optionId) => setSelectedPluginId(Number(optionId))}
            selectPlaceholder="选择插件"
        />
    );
};

export default PluginCenterConfig;
