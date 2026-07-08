import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";
import {
    CheckCircle2,
    ChevronDown,
    Download,
    ExternalLink,
    FolderOpen,
    Loader2,
    Plus,
    Power,
    PowerOff,
    Puzzle,
    RefreshCcw,
    RefreshCw,
    Trash2,
    XCircle,
} from "lucide-react";
import { Badge } from "../ui/badge";
import { Button } from "../ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../ui/card";
import { Input } from "../ui/input";
import { Textarea } from "../ui/textarea";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
} from "../ui/dialog";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator,
    DropdownMenuTrigger,
} from "../ui/dropdown-menu";
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
import { ConfigPageLayout, ListItemButton, SidebarList, type SelectOption } from "../common";
import { pluginRuntime } from "../../services/PluginRuntime";
import PluginViewHost from "../plugin/PluginViewHost";
import type { LoadedPlugin } from "../../services/PluginRuntime";

const PLUGIN_CENTER_VIEW_LOCATION = "config.plugin-center";
const DEFAULT_PLUGIN_DETAIL_TAB = "plugin-ui";

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
        bangs?: Array<{
            name: string;
            aliases?: string[];
            complete?: string | null;
            description?: string | null;
        }>;
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
        actions?: Array<{
            id: string;
            location: string;
            title: string;
            description?: string | null;
            order?: number | null;
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

interface PluginInstallRecipeSource {
    type: "github" | "zip" | "localZip";
    repo?: string | null;
    ref?: string;
    url?: string | null;
    path?: string | null;
}

interface PluginInstallRecipeDir {
    from: string;
    to: string;
}

interface OfficialPlugin {
    id: string;
    code: string;
    name: string;
    description: string;
    version?: string | null;
    author?: string | null;
    tags?: string[];
    pluginTypes: string[];
    permissions: string[];
    minAippVersion?: string | null;
    isExperimental?: boolean;
    source?: PluginInstallRecipeSource | null;
    dirs?: PluginInstallRecipeDir[];
    sourceUrl?: string | null;
    sha256?: string | null;
    isInstalled?: boolean;
    installedVersion?: string | null;
    isActive?: boolean;
}

interface PluginInstallValidation {
    isInstallable: boolean;
    warnings: string[];
    errors: string[];
}

interface PluginInstallPlanPlugin {
    from: string;
    to: string;
    code: string;
    name: string;
    version: string;
    description?: string | null;
    pluginType: string[];
    permissions: string[];
    willReplace: boolean;
    installedVersion?: string | null;
    validation: PluginInstallValidation;
}

interface PluginDetailItem {
    pluginId?: number | null;
    code: string;
    name: string;
    version: string;
    description?: string | null;
    author?: string | null;
    pluginType: string[];
    permissions: string[];
    runtime: {
        type: string;
        entry: string;
        protocol?: string | null;
        checksum?: string | null;
    };
    contributions?: PluginRegistryItem["contributions"];
    isInstalled: boolean;
    isActive: boolean;
    pluginDir: string;
    entryPath: string;
    entrySha256?: string | null;
    configs: PluginConfigItem[];
    hookRegistrations: PluginHookRegistrationItem[];
}

type OfficialPluginFetchStatus = "idle" | "loading" | "success" | "timeout" | "error";
type PluginArchiveInstallStatus = "idle" | "installing" | "installed" | "failed";

interface PluginArchiveRequest {
    source: PluginInstallRecipeSource;
    dirs?: PluginInstallRecipeDir[];
    expectedSha256?: string | null;
    useProxy: boolean;
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

function parsePluginSourceInput(input: string): {
    source: PluginInstallRecipeSource;
    dirs?: PluginInstallRecipeDir[];
    expectedSha256?: string | null;
} {
    const value = input.trim();
    if (!value) {
        throw new Error("请输入 GitHub 仓库或 ZIP 链接");
    }
    if (/^https?:\/\/.+\.zip(\?.*)?$/i.test(value)) {
        return { source: { type: "zip", url: value } };
    }
    const shorthandMatch = value.match(/^([\w.-]+\/[\w.-]+)(?:#(.+))?$/);
    if (shorthandMatch) {
        return {
            source: {
                type: "github",
                repo: shorthandMatch[1],
                ref: shorthandMatch[2] || "main",
            },
        };
    }
    const url = new URL(value);
    if (url.hostname !== "github.com") {
        throw new Error("只支持 GitHub 仓库链接或 ZIP 下载链接");
    }
    const segments = url.pathname.split("/").filter(Boolean);
    if (segments.length < 2) {
        throw new Error("GitHub 链接必须包含 owner/repo");
    }
    const repo = `${segments[0]}/${segments[1]}`;
    if (segments[2] === "tree" && segments[3]) {
        const ref = segments[3];
        const path = segments.slice(4).join("/");
        return {
            source: { type: "github", repo, ref },
            dirs: path
                ? [
                    {
                        from: path,
                        to: path.split("/").filter(Boolean).pop() || segments[1],
                    },
                ]
                : undefined,
        };
    }
    return { source: { type: "github", repo, ref: "main" } };
}

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
    const [pluginDetail, setPluginDetail] = useState<PluginDetailItem | null>(null);
    const [pluginDetailLoading, setPluginDetailLoading] = useState(false);
    const [actionBusy, setActionBusy] = useState(false);
    const [pendingUninstallPlugin, setPendingUninstallPlugin] = useState<PluginRegistryItem | null>(null);
    const [officialPlugins, setOfficialPlugins] = useState<OfficialPlugin[]>([]);
    const [officialFetchStatus, setOfficialFetchStatus] = useState<OfficialPluginFetchStatus>("idle");
    const [officialFetchError, setOfficialFetchError] = useState("");
    const [sourceInput, setSourceInput] = useState("");
    const [sourceInputError, setSourceInputError] = useState("");
    const [isInstallingArchive, setIsInstallingArchive] = useState(false);
    const [sourceInstallError, setSourceInstallError] = useState("");
    const [lastSourceInstallRequest, setLastSourceInstallRequest] = useState<PluginArchiveRequest | null>(null);
    const [officialInstallStatusMap, setOfficialInstallStatusMap] = useState<Record<string, PluginArchiveInstallStatus>>({});
    const [officialInstallErrorMap, setOfficialInstallErrorMap] = useState<Record<string, string>>({});
    const [installDialogOpen, setInstallDialogOpen] = useState(false);
    const [installDialogTab, setInstallDialogTab] = useState<"recommended" | "source">("recommended");
    const [activePluginDetailTab, setActivePluginDetailTab] = useState(DEFAULT_PLUGIN_DETAIL_TAB);
    const missingReminderKeyRef = useRef("");

    useEffect(() => {
        setRuntimePlugins(pluginList);
    }, [pluginList]);

    useEffect(() => {
        setActivePluginDetailTab(DEFAULT_PLUGIN_DETAIL_TAB);
    }, [selectedPluginId]);

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
                toast.warning(`发现插件目录缺失，请在插件中手动卸载：${preview}${suffix}`);
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

    const fetchOfficialPlugins = useCallback(async (useProxy = false) => {
        setOfficialFetchStatus("loading");
        setOfficialFetchError("");
        try {
            const items = await invoke<OfficialPlugin[]>("fetch_official_plugins", { useProxy });
            setOfficialPlugins(items);
            setOfficialFetchStatus("success");
        } catch (error) {
            const message = String(error);
            setOfficialFetchError(message);
            setOfficialFetchStatus(
                message.includes("超时") || message.toLowerCase().includes("timeout")
                    ? "timeout"
                    : "error"
            );
        }
    }, []);

    const handleSelectLocalZip = useCallback(async () => {
        try {
            const selected = await open({
                multiple: false,
                directory: false,
                filters: [
                    {
                        name: "ZIP 插件包",
                        extensions: ["zip"],
                    },
                ],
            });
            if (!selected || typeof selected !== "string") {
                return;
            }
            setSourceInput(selected);
            setSourceInputError("");
        } catch (error) {
            toast.error("选择插件 ZIP 失败: " + error);
        }
    }, []);

    const installPluginArchiveRequest = useCallback(
        async (request: PluginArchiveRequest) => {
            const result = await invoke<{ installedPlugins: PluginInstallPlanPlugin[] }>(
                "install_plugin_archive_source",
                {
                    source: request.source,
                    selections: request.dirs && request.dirs.length > 0 ? request.dirs : [],
                    expectedSha256: request.expectedSha256 || null,
                    useProxy: request.useProxy,
                    enableAfterInstall: true,
                }
            );
            await loadPlugins();
            return result;
        },
        [loadPlugins]
    );

    const handleInstallOfficialPlugin = useCallback(
        async (plugin: OfficialPlugin, useProxy = false) => {
            if (!plugin.source) {
                toast.error(`推荐插件 ${plugin.name} 缺少安装来源`);
                return;
            }

            const request: PluginArchiveRequest = {
                source: plugin.source,
                dirs: plugin.dirs,
                expectedSha256: plugin.sha256 || null,
                useProxy,
            };

            setOfficialInstallStatusMap((prev) => ({ ...prev, [plugin.id]: "installing" }));
            setOfficialInstallErrorMap((prev) => {
                const next = { ...prev };
                delete next[plugin.id];
                return next;
            });

            try {
                const result = await installPluginArchiveRequest(request);
                setOfficialInstallStatusMap((prev) => ({ ...prev, [plugin.id]: "installed" }));
                setOfficialPlugins((prev) =>
                    prev.map((item) =>
                        item.id === plugin.id
                            ? {
                                ...item,
                                isInstalled: true,
                                isActive: true,
                                installedVersion: result.installedPlugins[0]?.version ?? item.installedVersion,
                            }
                            : item
                    )
                );
                toast.success(`已安装 ${result.installedPlugins.length} 个插件`);
            } catch (error) {
                const message = String(error instanceof Error ? error.message : error);
                setOfficialInstallStatusMap((prev) => ({ ...prev, [plugin.id]: "failed" }));
                setOfficialInstallErrorMap((prev) => ({ ...prev, [plugin.id]: message }));
            }
        },
        [installPluginArchiveRequest]
    );

    const installCustomSource = useCallback(async (request: PluginArchiveRequest) => {
        setIsInstallingArchive(true);
        setSourceInstallError("");
        setLastSourceInstallRequest(request);
        try {
            const result = await installPluginArchiveRequest(request);
            toast.success(`已安装 ${result.installedPlugins.length} 个插件`);
            setInstallDialogOpen(false);
        } catch (error) {
            const message = String(error instanceof Error ? error.message : error);
            setSourceInstallError(message);
            toast.error("安装插件失败: " + message);
        } finally {
            setIsInstallingArchive(false);
        }
    }, [installPluginArchiveRequest]);

    const handleInstallCustomSource = useCallback(
        async (useProxy = false) => {
            let parsed;
            try {
                parsed = parsePluginSourceInput(sourceInput);
                setSourceInputError("");
            } catch (error) {
                const message = String(error instanceof Error ? error.message : error);
                setSourceInputError(message);
                return;
            }

            await installCustomSource({
                source: parsed.source,
                dirs: parsed.dirs,
                expectedSha256: parsed.expectedSha256 || null,
                useProxy,
            });
        },
        [installCustomSource, sourceInput]
    );

    const handleRetryCustomSourceInstall = useCallback(
        async (useProxy: boolean) => {
            if (!lastSourceInstallRequest) {
                return;
            }
            await installCustomSource({
                ...lastSourceInstallRequest,
                useProxy,
            });
        },
        [installCustomSource, lastSourceInstallRequest]
    );

    const handleShowInstallDialog = useCallback(() => {
        setInstallDialogTab("recommended");
        setInstallDialogOpen(true);
    }, []);

    const handleCloseInstallDialog = useCallback((open: boolean) => {
        setInstallDialogOpen(open);
        if (!open) {
            setSourceInputError("");
            setSourceInstallError("");
            setLastSourceInstallRequest(null);
        }
    }, []);

    const handleOpenPluginFolder = useCallback(async () => {
        try {
            const pluginRoot = await invoke<string>("get_plugin_root_dir");
            await openPath(pluginRoot);
        } catch (error) {
            toast.error("打开插件文件夹失败: " + error);
        }
    }, []);

    const getOfficialPluginRepositoryUrl = useCallback((plugin: OfficialPlugin) => {
        if (plugin.sourceUrl) {
            return plugin.sourceUrl;
        }
        if (plugin.source?.type === "github" && plugin.source.repo) {
            return `https://github.com/${plugin.source.repo}`;
        }
        return null;
    }, []);

    const handleOpenOfficialPluginRepository = useCallback(
        async (plugin: OfficialPlugin) => {
            const url = getOfficialPluginRepositoryUrl(plugin);
            if (!url) {
                toast.error(`${plugin.name} 缺少仓库链接`);
                return;
            }
            try {
                await invoke("open_source_url", { url });
            } catch (error) {
                toast.error("打开插件仓库失败: " + error);
            }
        },
        [getOfficialPluginRepositoryUrl]
    );

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

    const loadPluginDetail = useCallback(async (code: string) => {
        setPluginDetailLoading(true);
        try {
            const detail = await invoke<PluginDetailItem>("get_plugin_detail", { code });
            setPluginDetail(detail);
        } catch (error) {
            console.error("[PluginCenterConfig] Failed to load plugin detail:", error);
            setPluginDetail(null);
            toast.error("加载插件详情失败");
        } finally {
            setPluginDetailLoading(false);
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
            const selected = plugins.find((item) => item.pluginId === selectedPluginId);
            loadConfigs(selectedPluginId);
            loadHookDebugInfo(selectedPluginId);
            if (selected) {
                loadPluginDetail(selected.code);
            }
        } else {
            setConfigs([]);
            setHookRegistrations([]);
            setHookAuditLogs([]);
            setPluginDetail(null);
        }
    }, [selectedPluginId, plugins, loadConfigs, loadHookDebugInfo, loadPluginDetail]);

    useEffect(() => {
        if (installDialogOpen && officialFetchStatus === "idle") {
            fetchOfficialPlugins(false);
        }
    }, [fetchOfficialPlugins, installDialogOpen, officialFetchStatus]);

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
    const pluginCenterViews = useMemo(
        () =>
            (selectedPlugin?.contributions?.views ?? []).filter(
                (view) => view.location === PLUGIN_CENTER_VIEW_LOCATION
            ),
        [selectedPlugin]
    );
    const hasPluginCenterViews = pluginCenterViews.length > 0;
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
    const selectedRuntimeType = selectedPlugin?.runtime?.type ?? "js";
    const pluginViewBlockedReason = useMemo(() => {
        if (!selectedPlugin) {
            return "请选择一个插件。";
        }
        if (!hasPluginCenterViews) {
            return "当前插件没有声明插件界面。";
        }
        if (!selectedPlugin.isInstalled) {
            return "插件目录缺失，请先卸载该记录。";
        }
        if (!selectedPlugin.isActive) {
            return "插件已禁用，启用后可使用插件界面。";
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
        if (typeof currentLoadedPlugin.instance?.renderView !== "function") {
            return "插件已加载，但未实现 renderView()。";
        }
        return null;
    }, [selectedPlugin, currentLoadedPlugin, hasPluginCenterViews, selectedRuntimeType]);
    const canRenderPluginViews = !pluginViewBlockedReason;

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

    const pluginDetailContent = pluginDetailLoading ? (
        <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
            加载插件详情中...
        </div>
    ) : !pluginDetail ? (
        <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
            暂无插件详情。
        </div>
    ) : (
        <div className="space-y-4">
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
                <div className="rounded-md border border-border p-3 text-sm">
                    <div className="font-medium">安装目录</div>
                    <div className="mt-1 break-all text-muted-foreground">{pluginDetail.pluginDir}</div>
                </div>
                <div className="rounded-md border border-border p-3 text-sm">
                    <div className="font-medium">入口文件</div>
                    <div className="mt-1 break-all text-muted-foreground">{pluginDetail.entryPath}</div>
                    {pluginDetail.entrySha256 && (
                        <div className="mt-1 break-all text-xs text-muted-foreground">
                            {pluginDetail.entrySha256}
                        </div>
                    )}
                </div>
            </div>
            <div className="rounded-md border border-border p-3 text-sm">
                <div className="font-medium">Runtime</div>
                <div className="mt-2 flex flex-wrap gap-2">
                    <Badge variant="outline">{pluginDetail.runtime.type}</Badge>
                    <Badge variant="outline">{pluginDetail.runtime.entry}</Badge>
                    {pluginDetail.runtime.checksum && (
                        <Badge variant="secondary">{pluginDetail.runtime.checksum}</Badge>
                    )}
                </div>
            </div>
            <div className="rounded-md border border-border p-3 text-sm">
                <div className="font-medium">权限</div>
                <div className="mt-2 flex flex-wrap gap-1.5">
                    {pluginDetail.permissions.length === 0 ? (
                        <span className="text-muted-foreground">未声明权限</span>
                    ) : (
                        pluginDetail.permissions.map((permission) => (
                            <Badge key={permission} variant="secondary">
                                {permission}
                            </Badge>
                        ))
                    )}
                </div>
            </div>
            <div className="rounded-md border border-border p-3 text-sm">
                <div className="font-medium">贡献点</div>
                <div className="mt-2 grid grid-cols-2 gap-2 md:grid-cols-5">
                    <Badge variant="outline">Bangs {pluginDetail.contributions?.bangs?.length ?? 0}</Badge>
                    <Badge variant="outline">Hooks {pluginDetail.contributions?.hooks?.length ?? 0}</Badge>
                    <Badge variant="outline">视图 {pluginDetail.contributions?.views?.length ?? 0}</Badge>
                    <Badge variant="outline">Actions {pluginDetail.contributions?.actions?.length ?? 0}</Badge>
                    <Badge variant="outline">
                        Fields {pluginDetail.contributions?.assistantFormFields?.length ?? 0}
                    </Badge>
                </div>
            </div>
        </div>
    );

    const recommendedPluginsContent = (
        <div className="space-y-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
                <div>
                    <div className="text-sm font-medium">官方推荐插件</div>
                    <div className="text-xs text-muted-foreground">
                        从官方接口获取预编译插件包，并下载安装到本地插件目录。
                    </div>
                </div>
                <div className="flex gap-2">
                    <Button variant="outline" size="sm" onClick={() => fetchOfficialPlugins(false)} disabled={officialFetchStatus === "loading"}>
                        <RefreshCcw className={`h-4 w-4 ${officialFetchStatus === "loading" ? "animate-spin" : ""}`} />
                        <span className="ml-2">刷新</span>
                    </Button>
                    {officialFetchStatus === "timeout" || officialFetchStatus === "error" ? (
                        <Button variant="outline" size="sm" onClick={() => fetchOfficialPlugins(true)}>
                            使用代理重试
                        </Button>
                    ) : null}
                </div>
            </div>
            {officialFetchStatus === "idle" ? (
                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                    点击刷新获取官方推荐插件。
                </div>
            ) : officialFetchStatus === "loading" ? (
                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                    正在获取推荐插件...
                </div>
            ) : officialFetchError ? (
                <div className="rounded-md border border-destructive/40 p-3 text-sm text-destructive">
                    {officialFetchError}
                </div>
            ) : officialPlugins.length === 0 ? (
                <div className="rounded-md border border-border p-3 text-sm text-muted-foreground">
                    暂无官方推荐插件。
                </div>
            ) : (
                <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
                    {officialPlugins.map((plugin) => {
                        const status = officialInstallStatusMap[plugin.id] ?? "idle";
                        const error = officialInstallErrorMap[plugin.id];
                        const isBusy = status === "installing";
                        const hasSource = !!plugin.source;
                        const repositoryUrl = getOfficialPluginRepositoryUrl(plugin);

                        return (
                            <Card key={plugin.id} className="shadow-none">
                                <CardHeader>
                                    <div className="flex items-start justify-between gap-3">
                                        <div>
                                            <CardTitle className="text-base">{plugin.name}</CardTitle>
                                            <CardDescription className="mt-1">{plugin.description}</CardDescription>
                                        </div>
                                        <div className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
                                            {plugin.isInstalled && <Badge variant="secondary">已安装</Badge>}
                                            {status === "installed" && (
                                                <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                                                    <CheckCircle2 className="h-3.5 w-3.5" />
                                                    已完成
                                                </span>
                                            )}
                                        </div>
                                    </div>
                                </CardHeader>
                                <CardContent className="space-y-3">
                                    <div className="flex flex-wrap gap-1.5">
                                        {plugin.version && <Badge variant="outline">v{plugin.version}</Badge>}
                                        {plugin.isExperimental && <Badge variant="secondary">实验性</Badge>}
                                        {plugin.pluginTypes.map((type) => (
                                            <Badge key={type} variant="outline">
                                                {getPluginTypeLabel(type)}
                                            </Badge>
                                        ))}
                                    </div>
                                    {plugin.permissions.length > 0 && (
                                        <div className="flex flex-wrap gap-1.5">
                                            {plugin.permissions.map((permission) => (
                                                <Badge key={permission} variant="secondary">
                                                    {permission}
                                                </Badge>
                                            ))}
                                        </div>
                                    )}
                                    <div className="flex flex-wrap gap-2">
                                        <Button
                                            size="sm"
                                            onClick={() => handleInstallOfficialPlugin(plugin)}
                                            disabled={!hasSource || isBusy || status === "installed"}
                                            className="gap-1.5"
                                        >
                                            {isBusy ? (
                                                <Loader2 className="h-3.5 w-3.5 animate-spin" />
                                            ) : status === "installed" ? (
                                                <CheckCircle2 className="h-3.5 w-3.5" />
                                            ) : (
                                                <Download className="h-3.5 w-3.5" />
                                            )}
                                            {status === "installing"
                                                ? "安装中"
                                                : status === "installed"
                                                    ? "已完成"
                                                    : plugin.isInstalled
                                                        ? "重新安装"
                                                        : "安装"}
                                        </Button>
                                        <Button
                                            variant="outline"
                                            size="sm"
                                            onClick={() => handleOpenOfficialPluginRepository(plugin)}
                                            disabled={!repositoryUrl}
                                            className="gap-1.5"
                                        >
                                            <ExternalLink className="h-3.5 w-3.5" />
                                            前往仓库
                                        </Button>
                                    </div>
                                    {status === "failed" && error && (
                                        <div className="flex items-start gap-2 text-xs text-destructive">
                                            <XCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                                            <div className="min-w-0 flex-1 space-y-1">
                                                <div className="break-words">{error}</div>
                                                <div className="flex flex-wrap gap-2 pt-1">
                                                    <Button
                                                        variant="outline"
                                                        size="sm"
                                                        className="h-6 text-xs"
                                                        onClick={() => handleInstallOfficialPlugin(plugin)}
                                                    >
                                                        重试
                                                    </Button>
                                                    <Button
                                                        variant="outline"
                                                        size="sm"
                                                        className="h-6 gap-1 text-xs"
                                                        onClick={() => handleInstallOfficialPlugin(plugin, true)}
                                                    >
                                                        <RefreshCw className="h-3 w-3" />
                                                        使用代理重试
                                                    </Button>
                                                </div>
                                            </div>
                                        </div>
                                    )}
                                </CardContent>
                            </Card>
                        );
                    })}
                </div>
            )}
        </div>
    );

    const sourceInstallContent = (
        <div className="space-y-4">
            <div>
                <div className="text-sm font-medium">从 GitHub / ZIP 安装</div>
                <div className="text-xs text-muted-foreground">
                    支持 owner/repo、owner/repo#ref、GitHub 仓库链接、GitHub tree 路径、ZIP 链接和本地 ZIP 文件。
                </div>
            </div>
            <div className="flex flex-col gap-2 md:flex-row">
                <Input
                    value={sourceInput}
                    onChange={(event) => setSourceInput(event.target.value)}
                    placeholder="owner/repo#main 或 https://example.com/plugin.zip"
                />
                <Button onClick={() => handleInstallCustomSource(false)} disabled={isInstallingArchive} className="gap-2">
                    {isInstallingArchive && <Loader2 className="h-4 w-4 animate-spin" />}
                    {isInstallingArchive ? "安装中" : "安装"}
                </Button>
                <Button variant="outline" onClick={() => handleInstallCustomSource(true)} disabled={isInstallingArchive}>
                    使用代理安装
                </Button>
                <Button variant="outline" onClick={handleSelectLocalZip} disabled={isInstallingArchive}>
                    选择 ZIP 文件
                </Button>
            </div>
            {sourceInputError && <div className="text-sm text-destructive">{sourceInputError}</div>}
            {sourceInstallError && (
                <div className="rounded-md border border-border bg-muted/30 p-3 text-sm">
                    <div className="font-medium">安装失败</div>
                    <div className="mt-1 break-words text-xs text-muted-foreground">{sourceInstallError}</div>
                    {lastSourceInstallRequest && (
                        <div className="mt-2 flex flex-wrap gap-2">
                            <Button
                                variant="outline"
                                size="sm"
                                onClick={() => handleRetryCustomSourceInstall(lastSourceInstallRequest.useProxy)}
                                disabled={isInstallingArchive}
                            >
                                重试安装
                            </Button>
                            {!lastSourceInstallRequest.useProxy && (
                                <Button
                                    variant="outline"
                                    size="sm"
                                    onClick={() => handleRetryCustomSourceInstall(true)}
                                    disabled={isInstallingArchive}
                                    className="gap-1"
                                >
                                    <RefreshCw className="h-3.5 w-3.5" />
                                    使用代理重试
                                </Button>
                            )}
                        </div>
                    )}
                </div>
            )}
        </div>
    );

    const pluginActionDropdown = (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <Button variant="outline" size="sm" className="gap-2" disabled={loading}>
                    <ChevronDown className="h-4 w-4" />
                </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end" className="w-56">
                <DropdownMenuItem
                    onClick={loadPlugins}
                    disabled={loading}
                    className="flex items-center gap-2 cursor-pointer"
                >
                    <div className="flex flex-col">
                        <span className="font-medium">扫描插件</span>
                        <span className="text-xs text-muted-foreground">扫描本地插件目录并刷新运行时</span>
                    </div>
                </DropdownMenuItem>
                <DropdownMenuSeparator />
                <DropdownMenuItem
                    onClick={handleShowInstallDialog}
                    className="flex items-center gap-2 cursor-pointer"
                >
                    <div className="flex flex-col">
                        <span className="font-medium">安装</span>
                        <span className="text-xs text-muted-foreground">从推荐列表、GitHub 或 ZIP 安装插件</span>
                    </div>
                </DropdownMenuItem>
                <DropdownMenuItem
                    onClick={handleOpenPluginFolder}
                    className="flex items-center gap-2 cursor-pointer"
                >
                    <div className="flex flex-col">
                        <span className="font-medium">打开插件文件夹</span>
                        <span className="text-xs text-muted-foreground">手动管理本地插件文件</span>
                    </div>
                </DropdownMenuItem>
            </DropdownMenuContent>
        </DropdownMenu>
    );

    const sidebar = (
        <SidebarList
            title="插件"
            description="统一管理已安装插件及其配置"
            icon={<Puzzle className="h-5 w-5" />}
            searchValue={searchQuery}
            onSearchChange={setSearchQuery}
            searchPlaceholder="搜索插件..."
            addButton={pluginActionDropdown}
        >
            {filteredPlugins.map((plugin) => (
                <ListItemButton
                    key={plugin.pluginId}
                    isSelected={selectedPluginId === plugin.pluginId}
                    onClick={() => setSelectedPluginId(plugin.pluginId)}
                >
                    <div className="flex items-center w-full">
                        <div className="flex-1 truncate">
                            <div
                                className={`font-medium truncate ${!plugin.isInstalled ? "text-destructive" : ""}`}
                            >
                                {plugin.isInstalled
                                    ? plugin.name
                                    : `${plugin.name}（目录缺失）`}
                            </div>
                        </div>
                    </div>
                </ListItemButton>
            ))}
        </SidebarList>
    );

    const content = !selectedPlugin ? (
        <Card className="shadow-none">
            <CardHeader>
                <CardTitle className="text-lg">插件商店</CardTitle>
                <CardDescription>从左侧选择插件查看详情，或通过右上角菜单扫描、安装和打开插件文件夹。</CardDescription>
            </CardHeader>
            <CardContent className="flex flex-wrap gap-2">
                <Button onClick={handleShowInstallDialog}>
                    <Plus className="h-4 w-4" />
                    <span className="ml-2">安装插件</span>
                </Button>
                <Button variant="outline" onClick={loadPlugins} disabled={loading}>
                    <RefreshCcw className={`h-4 w-4 ${loading ? "animate-spin" : ""}`} />
                    <span className="ml-2">扫描插件</span>
                </Button>
                <Button variant="outline" onClick={handleOpenPluginFolder}>
                    <FolderOpen className="h-4 w-4" />
                    <span className="ml-2">打开插件文件夹</span>
                </Button>
            </CardContent>
        </Card>
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
                                <Badge variant="outline">{selectedPlugin.contributions?.views?.length} 视图</Badge>
                            )}
                            {(selectedPlugin.contributions?.actions?.length ?? 0) > 0 && (
                                <Badge variant="outline">{selectedPlugin.contributions?.actions?.length} Actions</Badge>
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
                <Tabs value={activePluginDetailTab} onValueChange={setActivePluginDetailTab} className="w-full">
                    <TabsList>
                        <TabsTrigger value="plugin-ui">界面</TabsTrigger>
                        <TabsTrigger value="detail">详情</TabsTrigger>
                        <TabsTrigger value="config">配置KV</TabsTrigger>
                        <TabsTrigger value="hooks">Hook 调试</TabsTrigger>
                    </TabsList>
                    <TabsContent value="plugin-ui" className="mt-4 space-y-3">
                        {canRenderPluginViews ? (
                            <PluginViewHost
                                pluginList={selectedPluginViewHostItems}
                                location={PLUGIN_CENTER_VIEW_LOCATION}
                                emptyDescription="当前插件没有声明插件界面。"
                            />
                        ) : (
                            <div className="rounded-lg border border-border/60 bg-background p-3 md:p-4">
                                <div className="text-sm text-muted-foreground">
                                    {pluginViewBlockedReason}
                                </div>
                            </div>
                        )}
                        {selectedPlugin.isInstalled &&
                            selectedPlugin.isActive &&
                            hasPluginCenterViews && (
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
                    <TabsContent value="detail" className="mt-4">
                        {pluginDetailContent}
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
                                <div className="text-sm text-muted-foreground">
                                    暂无配置项。
                                </div>
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
            <Dialog open={installDialogOpen} onOpenChange={handleCloseInstallDialog}>
                <DialogContent className="flex h-[85vh] max-h-[85vh] flex-col overflow-hidden sm:max-w-5xl">
                    <DialogHeader>
                        <DialogTitle>安装插件</DialogTitle>
                        <DialogDescription>
                            安装官方推荐插件，或从 GitHub / ZIP 来源安装已经编译好的插件包。
                        </DialogDescription>
                    </DialogHeader>
                    <Tabs
                        value={installDialogTab}
                        onValueChange={(value) => setInstallDialogTab(value as "recommended" | "source")}
                        className="flex min-h-0 flex-1 flex-col overflow-hidden"
                    >
                        <TabsList>
                            <TabsTrigger value="recommended">推荐插件</TabsTrigger>
                            <TabsTrigger value="source">来源安装</TabsTrigger>
                        </TabsList>
                        <TabsContent value="recommended" className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1">
                            {recommendedPluginsContent}
                        </TabsContent>
                        <TabsContent value="source" className="mt-4 min-h-0 flex-1 overflow-y-auto pr-1">
                            {sourceInstallContent}
                        </TabsContent>
                    </Tabs>
                </DialogContent>
            </Dialog>
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
