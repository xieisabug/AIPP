import React, { useCallback, useEffect, useState, useMemo } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { Button } from "../ui/button";
import { Switch } from "../ui/switch";
import { Badge } from "../ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "../ui/tabs";
import { Trash2, Edit, RefreshCw, Zap } from "lucide-react";
import MCP from "@/assets/mcp.svg?react";
import { Tooltip, TooltipTrigger, TooltipContent } from "../ui/tooltip";
import { toast } from 'sonner';
import { listen } from "@tauri-apps/api/event";
import ConfirmDialog from "../ConfirmDialog";
import MCPServerDialog from "./MCPServerDialog";
import MCPActionDropdown from "./MCPActionDropdown";
import JSONImportDialog from "./JSONImportDialog";
import MCPToolItem from "./MCPToolItem";
import BuiltinToolDialog from "./BuiltinToolDialog";

// 导入公共组件
import {
    ConfigPageLayout,
    SidebarList,
    ListItemButton,
    EmptyState,
    SelectOption
} from "../common";

import { MCPServer, MCPServerTool, MCPServerResource, MCPServerPrompt, MCPServerRequest } from "../../data/MCP";
import { MCPTemplate } from "../../data/MCPTemplates";
import { useSkillsMcpValidation, DisableOperationMcpCheckResult, AGENT_MCP_COMMAND } from "../../hooks/useSkillsMcpValidation";

const MCPConfig: React.FC = () => {
    const [mcpServers, setMcpServers] = useState<MCPServer[]>([]);
    const [selectedServer, setSelectedServer] = useState<MCPServer | null>(null);
    const [serverTools, setServerTools] = useState<MCPServerTool[]>([]);
    const [serverResources, setServerResources] = useState<MCPServerResource[]>([]);
    const [serverPrompts, setServerPrompts] = useState<MCPServerPrompt[]>([]);

    // Dialog states
    const [serverDialogOpen, setServerDialogOpen] = useState(false);
    const [editingServer, setEditingServer] = useState<MCPServer | null>(null);
    const [jsonImportDialogOpen, setJsonImportDialogOpen] = useState(false);
    const [confirmDialogOpen, setConfirmDialogOpen] = useState(false);
    const [builtinDialogOpen, setBuiltinDialogOpen] = useState(false);
    const [deletingServerId, setDeletingServerId] = useState<number | null>(null);
    const [builtinEditOpen, setBuiltinEditOpen] = useState(false);
    const [builtinEditEnv, setBuiltinEditEnv] = useState<string>("");
    const [builtinEditName, setBuiltinEditName] = useState<string>("");

    // Dialog initial data
    const [dialogInitialServerType, setDialogInitialServerType] = useState<string | undefined>(undefined);
    const [dialogInitialConfig, setDialogInitialConfig] = useState<Partial<MCPServerRequest> | undefined>(undefined);

    // Loading states
    const [isRefreshing, setIsRefreshing] = useState(false);

    // Tool expansion states
    const [expandedTools, setExpandedTools] = useState<Set<number>>(new Set());

    // Skills/MCP 联动校验
    const { checkDisableAgentMcp, disableAgentMcpWithSkills } = useSkillsMcpValidation();

    // 关闭操作 MCP 确认对话框
    const [disableOperationMcpConfirmOpen, setDisableOperationMcpConfirmOpen] = useState(false);
    const [disableOperationMcpCheckResult, setDisableOperationMcpCheckResult] = useState<DisableOperationMcpCheckResult | null>(null);
    const [pendingServerToggle, setPendingServerToggle] = useState<{ serverId: number; isEnabled: boolean } | null>(null);

    // Toggle tool expansion
    const toggleToolExpansion = useCallback((toolId: number) => {
        setExpandedTools(prev => {
            const newSet = new Set(prev);
            if (newSet.has(toolId)) {
                newSet.delete(toolId);
            } else {
                newSet.add(toolId);
            }
            return newSet;
        });
    }, []);

    // Utility function to truncate text to specified number of lines
    const truncateText = useCallback((text: string, maxLines: number = 2) => {
        if (!text) return '';
        const words = text.split(' ');
        const maxWordsPerLine = 12; // Approximate words per line
        const maxWords = maxWordsPerLine * maxLines;

        if (words.length <= maxWords) return text;
        return words.slice(0, maxWords).join(' ') + '...';
    }, []);

    // 获取MCP服务器列表
    const getMcpServers = useCallback(() => {
        invoke<MCPServer[]>('get_mcp_servers')
            .then((servers) => {
                setMcpServers(servers);
                // 如果没有选中的服务器，选择第一个
                if (!selectedServer && servers.length > 0) {
                    setSelectedServer(servers[0]);
                }
                // 如果当前选中的服务器已被删除，选择第一个
                if (selectedServer && !servers.find(s => s.id === selectedServer.id)) {
                    setSelectedServer(servers.length > 0 ? servers[0] : null);
                }
            })
            .catch((e) => {
                toast.error('获取MCP服务器失败: ' + e);
            });
    }, [selectedServer]);

    // 获取服务器工具列表
    const getServerTools = useCallback((serverId: number) => {
        invoke<MCPServerTool[]>('get_mcp_server_tools', { serverId })
            .then(setServerTools)
            .catch((e) => {
                toast.error('获取服务器工具失败: ' + e);
            });
    }, []);

    // 获取服务器资源列表
    const getServerResources = useCallback((serverId: number) => {
        invoke<MCPServerResource[]>('get_mcp_server_resources', { serverId })
            .then(setServerResources)
            .catch((e) => {
                toast.error('获取服务器资源失败: ' + e);
            });
    }, []);

    // 获取服务器提示列表
    const getServerPrompts = useCallback((serverId: number) => {
        invoke<MCPServerPrompt[]>('get_mcp_server_prompts', { serverId })
            .then(setServerPrompts)
            .catch((e) => {
                toast.error('获取服务器提示失败: ' + e);
            });
    }, []);

    useEffect(() => {
        getMcpServers();

        // 监听后端 MCP 状态变更事件，自动刷新
        const unlistenPromise = listen('mcp_state_changed', () => {
            getMcpServers();
        });

        return () => {
            unlistenPromise.then(unlisten => unlisten());
        };
    }, []);

    useEffect(() => {
        if (selectedServer) {
            getServerTools(selectedServer.id);
            getServerResources(selectedServer.id);
            getServerPrompts(selectedServer.id);
        }
    }, [selectedServer, getServerTools, getServerResources, getServerPrompts]);

    // 切换服务器启用状态
    const handleToggleServer = useCallback(async (serverId: number, isEnabled: boolean) => {
        // 如果是关闭 Agent MCP，先检查是否有 Skills 依赖
        if (!isEnabled) {
            const server = mcpServers.find(s => s.id === serverId);
            if (server) {
                const isAgent = server.command === AGENT_MCP_COMMAND;
                if (isAgent) {
                    try {
                        const checkResult = await checkDisableAgentMcp();
                        if (checkResult.affected_assistants.length > 0) {
                            setDisableOperationMcpCheckResult(checkResult);
                            setPendingServerToggle({ serverId, isEnabled });
                            setDisableOperationMcpConfirmOpen(true);
                            return;
                        }
                    } catch (e) {
                        console.error('检查工具集依赖失败:', e);
                    }
                }
            }
        }

        try {
            await invoke('toggle_mcp_server', { id: serverId, isEnabled });
            setMcpServers(prev => prev.map(server =>
                server.id === serverId ? { ...server, is_enabled: isEnabled } : server
            ));
            if (selectedServer && selectedServer.id === serverId) {
                setSelectedServer(prev => prev ? { ...prev, is_enabled: isEnabled } : null);
            }
            toast.success(`服务器已${isEnabled ? '启用' : '禁用'}`);
        } catch (e) {
            toast.error('切换服务器状态失败: ' + e);
        }
    }, [selectedServer, mcpServers, checkDisableAgentMcp]);

    // 确认关闭操作 MCP 并同时关闭所有 Skills
    const handleConfirmDisableOperationMcp = useCallback(async () => {
        if (!pendingServerToggle) return;

        try {
            await disableAgentMcpWithSkills();
            
            // 更新 UI 状态
            setMcpServers(prev => prev.map(s =>
                s.id === pendingServerToggle.serverId ? { ...s, is_enabled: false } : s
            ));
            if (selectedServer && selectedServer.id === pendingServerToggle.serverId) {
                setSelectedServer(prev => prev ? { ...prev, is_enabled: false } : null);
                // 清空已选服务器的能力数据，避免显示旧的启用数量
                setServerTools([]);
                setServerResources([]);
                setServerPrompts([]);
            }
            // 重新获取列表，确保显示的启用数量更新
            getMcpServers();

            toast.success('已关闭 Agent 工具集和相关Skills');
        } catch (e) {
            toast.error('关闭失败: ' + e);
        } finally {
            setDisableOperationMcpConfirmOpen(false);
            setDisableOperationMcpCheckResult(null);
            setPendingServerToggle(null);
        }
    }, [pendingServerToggle, selectedServer, disableAgentMcpWithSkills, getMcpServers]);

    // 打开新增服务器对话框
    const openAddServerDialog = useCallback((initialServerType?: string, initialConfig?: Partial<MCPServerRequest>) => {
        setEditingServer(null);
        setDialogInitialServerType(initialServerType);
        setDialogInitialConfig(initialConfig);
        setServerDialogOpen(true);
    }, []);

    // 打开编辑服务器对话框
    const openEditServerDialog = useCallback((server: MCPServer) => {
        // Built-in servers (identified by is_builtin or aipp:* command) use env-only dialog
        const isBuiltin = server.is_builtin || (!!server.command && server.command.startsWith('aipp:'));
        if (isBuiltin) {
            setEditingServer(server);
            setBuiltinEditEnv(server.environment_variables || "");
            setBuiltinEditName(server.name || "");
            setBuiltinEditOpen(true);
            return;
        }
        setEditingServer(server);
        setServerDialogOpen(true);
    }, []);

    // 关闭服务器对话框
    const closeServerDialog = useCallback(() => {
        setServerDialogOpen(false);
        setEditingServer(null);
        setDialogInitialServerType(undefined);
        setDialogInitialConfig(undefined);
    }, []);

    // 服务器对话框提交
    const handleServerDialogSubmit = useCallback(() => {
        closeServerDialog();
        getMcpServers();
    }, [closeServerDialog, getMcpServers]);

    // 删除服务器
    const handleDeleteServer = useCallback((serverId: number) => {
        setDeletingServerId(serverId);
        setConfirmDialogOpen(true);
    }, []);

    const confirmDeleteServer = useCallback(async () => {
        if (!deletingServerId) return;

        try {
            await invoke('delete_mcp_server', { id: deletingServerId });
            toast.success('删除服务器成功');
            getMcpServers();
        } catch (e) {
            toast.error('删除服务器失败: ' + e);
        }

        setConfirmDialogOpen(false);
        setDeletingServerId(null);
    }, [deletingServerId, getMcpServers]);

    // 选择服务器
    const handleSelectServer = useCallback((server: MCPServer) => {
        setSelectedServer(server);
    }, []);

    // 刷新服务器能力
    const handleRefreshServerCapabilities = useCallback(async (serverId: number) => {
        setIsRefreshing(true);
        try {
            const [tools, resources, prompts] = await invoke<[MCPServerTool[], MCPServerResource[], MCPServerPrompt[]]>(
                'refresh_mcp_server_capabilities',
                { serverId }
            );
            setServerTools(tools);
            setServerResources(resources);
            setServerPrompts(prompts);
            toast.success('服务器能力刷新成功');
        } catch (e) {
            toast.error('刷新服务器能力失败: ' + e);
        } finally {
            setIsRefreshing(false);
        }
    }, []);

    // 更新工具配置
    const handleUpdateTool = useCallback(async (toolId: number, isEnabled: boolean, isAutoRun: boolean) => {
        // 关闭 Agent 的 load_skill 时先确认
        if (!isEnabled && selectedServer?.command === AGENT_MCP_COMMAND) {
            const tool = serverTools.find(t => t.id === toolId);
            if (tool && tool.tool_name === 'load_skill') {
                try {
                    const checkResult = await checkDisableAgentMcp();
                    if (checkResult.affected_assistants.length > 0) {
                        setDisableOperationMcpCheckResult(checkResult);
                        setPendingServerToggle({ serverId: selectedServer.id, isEnabled: false });
                        setDisableOperationMcpConfirmOpen(true);
                        return;
                    }
                } catch (e) {
                    console.error('检查 Agent 依赖失败:', e);
                }
            }
        }

        try {
            await invoke('update_mcp_server_tool', { toolId, isEnabled, isAutoRun });
            setServerTools(prev => prev.map(tool =>
                tool.id === toolId ? { ...tool, is_enabled: isEnabled, is_auto_run: isAutoRun } : tool
            ));
        } catch (e) {
            toast.error('更新工具配置失败: ' + e);
        }
    }, [selectedServer, serverTools, checkDisableAgentMcp]);

    // 更新提示配置
    const handleUpdatePrompt = useCallback(async (promptId: number, isEnabled: boolean) => {
        try {
            await invoke('update_mcp_server_prompt', { promptId, isEnabled });
            setServerPrompts(prev => prev.map(prompt =>
                prompt.id === promptId ? { ...prompt, is_enabled: isEnabled } : prompt
            ));
        } catch (e) {
            toast.error('更新提示配置失败: ' + e);
        }
    }, []);

    // 处理模板选择
    const handleTemplateSelect = useCallback((template: MCPTemplate) => {
        if (template.id === 'builtin-search') {
            setBuiltinDialogOpen(true);
            return;
        }
        openAddServerDialog(template.template.transport_type, template.template);
    }, [openAddServerDialog]);

    // 处理JSON导入
    const handleJSONImport = useCallback(() => {
        setJsonImportDialogOpen(true);
    }, []);

    // 处理JSON导入确认  
    const handleJSONImportConfirm = useCallback(async (configs: MCPServerRequest[]) => {
        setJsonImportDialogOpen(false);

        // 如果只有一个服务器，直接打开对话框编辑
        if (configs.length === 1) {
            openAddServerDialog(configs[0].transport_type, configs[0]);
            return;
        }

        // 多个服务器，批量创建
        let successCount = 0;
        let errorCount = 0;

        for (const config of configs) {
            try {
                const serverId = await invoke<number>('add_mcp_server', { request: config });
                successCount++;

                // 尝试自动获取能力
                try {
                    await invoke('refresh_mcp_server_capabilities', { serverId });
                } catch (e) {
                    console.warn('自动获取能力失败:', e);
                }
            } catch (e) {
                console.error('创建服务器失败:', e);
                errorCount++;
            }
        }

        if (successCount > 0) {
            toast.success(`成功创建 ${successCount} 个MCP服务器`);
            getMcpServers(); // 刷新服务器列表
        }

        if (errorCount > 0) {
            toast.error(`${errorCount} 个服务器创建失败`);
        }
    }, [openAddServerDialog, getMcpServers]);

    // 关闭JSON导入对话框
    const closeJSONImportDialog = useCallback(() => {
        setJsonImportDialogOpen(false);
    }, []);

    // 下拉菜单选项
    const selectOptions: SelectOption[] = useMemo(() =>
        mcpServers.map(server => ({
            id: server.id.toString(),
            label: server.name,
            icon: server.is_enabled ? <Zap className="h-4 w-4" /> : <MCP className="h-4 w-4" />
        })), [mcpServers]);

    // 下拉菜单选择回调
    const handleSelectFromDropdown = useCallback((serverId: string) => {
        const server = mcpServers.find(s => s.id.toString() === serverId);
        if (server) {
            handleSelectServer(server);
        }
    }, [mcpServers, handleSelectServer]);

    // 新增按钮组件
    const addButton = useMemo(() => (
        <MCPActionDropdown
            onTemplateSelect={handleTemplateSelect}
            onJSONImport={handleJSONImport}
            className="bg-primary hover:bg-primary/90 text-primary-foreground shadow-sm hover:shadow-md transition-all"
        />
    ), [handleTemplateSelect, handleJSONImport]);

    // 侧边栏内容 - 使用 useMemo 避免重复创建（必须在条件返回之前）
    const sidebar = useMemo(() => (
        <SidebarList
            title="MCP列表"
            description="选择MCP进行配置"
            icon={<MCP className="h-5 w-5" />}
            addButton={
                <MCPActionDropdown
                    onTemplateSelect={handleTemplateSelect}
                    onJSONImport={handleJSONImport}
                    variant="outline"
                    size="sm"
                    showIcon={false}
                />
            }
        >
            {mcpServers.map((server) => (
                <ListItemButton
                    key={server.id}
                    isSelected={selectedServer?.id === server.id}
                    onClick={() => handleSelectServer(server)}
                >
                    <div className="flex items-center w-full">
                        <div className="flex-1 truncate">
                            <div className="font-medium truncate">{server.name}</div>
                        </div>
                        {server.is_enabled && (
                            <Zap className="h-3 w-3 ml-2 flex-shrink-0" />
                        )}
                    </div>
                </ListItemButton>
            ))}
        </SidebarList>
    ), [mcpServers, selectedServer?.id, handleSelectServer, handleTemplateSelect, handleJSONImport]);

    // 右侧内容 - 使用 useMemo 避免重复创建（必须在条件返回之前）
    const content = useMemo(() => selectedServer ? (
        <div className="space-y-6">
            {/* 服务器基本信息 */}
            <div className="bg-background rounded-lg border border-border p-6">
                <div className="flex items-center justify-between mb-4">
                    <div className='mr-8 mb-5'>
                        <h3 className="text-lg font-semibold text-foreground">{selectedServer.name}</h3>
                        <p className="text-sm text-muted-foreground">{selectedServer.description || '暂无描述'}</p>
                    </div>
                    <div className="flex items-center gap-1">
                        <Switch
                            checked={selectedServer.is_enabled}
                            onCheckedChange={(checked) => handleToggleServer(selectedServer.id, checked)}
                        />
                        <Tooltip delayDuration={500}>
                            <TooltipTrigger asChild>
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => handleRefreshServerCapabilities(selectedServer.id)}
                                    disabled={isRefreshing}
                                >
                                    <RefreshCw className={`h-4 w-4 ${isRefreshing ? 'animate-spin' : ''}`} />
                                </Button>
                            </TooltipTrigger>
                            <TooltipContent>重新获取能力</TooltipContent>
                        </Tooltip>

                        <Tooltip delayDuration={500}>
                            <TooltipTrigger asChild>
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => openEditServerDialog(selectedServer)}
                                >
                                    <Edit className="h-4 w-4" />
                                </Button>
                            </TooltipTrigger>
                            <TooltipContent>编辑MCP</TooltipContent>
                        </Tooltip>

                        {/* 系统内置工具集不显示删除按钮（is_deletable = false） */}
                        {selectedServer.is_deletable && (
                            <Tooltip delayDuration={500}>
                                <TooltipTrigger asChild>
                                    <Button
                                        variant="ghost"
                                        size="sm"
                                        onClick={() => handleDeleteServer(selectedServer.id)}
                                    >
                                        <Trash2 className="h-4 w-4 text-destructive" />
                                    </Button>
                                </TooltipTrigger>
                                <TooltipContent>删除MCP</TooltipContent>
                            </Tooltip>
                        )}
                    </div>
                </div>

                <div className="grid grid-cols-2 gap-4 text-sm">
                    <div>
                        <span className="font-medium text-foreground">传输类型:</span>
                        <Badge variant="secondary" className="ml-2">
                            {selectedServer.transport_type}
                        </Badge>
                    </div>
                    <div>
                        <span className="font-medium text-foreground">长期运行:</span>
                        <Badge variant={selectedServer.is_long_running ? "default" : "secondary"} className="ml-2">
                            {selectedServer.is_long_running ? "是" : "否"}
                        </Badge>
                    </div>
                    {selectedServer.timeout && (
                        <div>
                            <span className="font-medium text-foreground">超时时间:</span>
                            <span className="ml-2 text-muted-foreground">{selectedServer.timeout}ms</span>
                        </div>
                    )}
                </div>
            </div>

            {/* 能力列表 - 使用 Tabs */}
            <div className="bg-background rounded-lg border border-border p-6">
                <h4 className="text-md font-semibold text-foreground mb-4">MCP能力</h4>

                {/* 动态计算需要显示的tabs */}
                {(() => {
                    const availableTabs = [];
                    if (serverTools.length > 0) availableTabs.push("tools");
                    if (serverPrompts.length > 0) availableTabs.push("prompts");
                    if (serverResources.length > 0) availableTabs.push("resources");

                    const defaultValue = availableTabs.length > 0 ? availableTabs[0] : "tools";
                    const gridCols = availableTabs.length === 1 ? "grid-cols-1" :
                        availableTabs.length === 2 ? "grid-cols-2" : "grid-cols-3";

                    return availableTabs.length > 0 ? (
                        <Tabs defaultValue={defaultValue} className="w-full">
                            <TabsList className={`grid w-full ${gridCols}`}>
                                {serverTools.length > 0 && (
                                    <TabsTrigger value="tools">
                                        工具 ({serverTools.length})
                                    </TabsTrigger>
                                )}
                                {serverPrompts.length > 0 && (
                                    <TabsTrigger value="prompts">
                                        提示 ({serverPrompts.length})
                                    </TabsTrigger>
                                )}
                                {serverResources.length > 0 && (
                                    <TabsTrigger value="resources">
                                        资源 ({serverResources.length})
                                    </TabsTrigger>
                                )}
                            </TabsList>

                            {/* 工具列表 */}
                            {serverTools.length > 0 && (
                                <TabsContent value="tools" className="mt-4">
                                    <div className="space-y-3">
                                        {serverTools.map((tool) => (
                                            <MCPToolItem
                                                key={tool.id}
                                                tool={tool}
                                                isExpanded={expandedTools.has(tool.id)}
                                                onToggleExpansion={toggleToolExpansion}
                                                onUpdateTool={handleUpdateTool}
                                                truncateText={truncateText}
                                            />
                                        ))}
                                    </div>
                                </TabsContent>
                            )}

                            {/* 提示列表 */}
                            {serverPrompts.length > 0 && (
                                <TabsContent value="prompts" className="mt-4">
                                    <div className="space-y-3">
                                        {serverPrompts.map((prompt) => (
                                            <div key={prompt.id} className="flex items-center justify-between p-3 bg-muted rounded-lg">
                                                <div className="flex-1">
                                                    <div className="font-medium text-foreground">{prompt.prompt_name}</div>
                                                    {prompt.prompt_description && (
                                                        <div className="text-sm text-muted-foreground mt-1">{prompt.prompt_description}</div>
                                                    )}
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <span className="text-sm text-foreground">启用</span>
                                                    <Switch
                                                        checked={prompt.is_enabled}
                                                        onCheckedChange={(checked) =>
                                                            handleUpdatePrompt(prompt.id, checked)
                                                        }
                                                    />
                                                </div>
                                            </div>
                                        ))}
                                    </div>
                                </TabsContent>
                            )}

                            {/* 资源列表 */}
                            {serverResources.length > 0 && (
                                <TabsContent value="resources" className="mt-4">
                                    <div className="space-y-3">
                                        {serverResources.map((resource) => (
                                            <div key={resource.id} className="p-3 bg-muted rounded-lg">
                                                <div className="font-medium text-foreground">{resource.resource_name}</div>
                                                <div className="text-sm text-muted-foreground mt-1">{resource.resource_uri}</div>
                                                {resource.resource_description && (
                                                    <div className="text-sm text-muted-foreground mt-1">{resource.resource_description}</div>
                                                )}
                                                <Badge variant="outline" className="mt-2">{resource.resource_type}</Badge>
                                            </div>
                                        ))}
                                    </div>
                                </TabsContent>
                            )}

                        </Tabs>
                    ) : (
                        <div className="text-center py-8">
                            <MCP className="h-12 w-12 text-muted-foreground mx-auto mb-4" />
                            <p className="text-sm text-muted-foreground">暂无能力数据</p>
                            <p className="text-xs text-muted-foreground mt-1">点击上方"刷新能力"按钮获取服务器能力</p>
                        </div>
                    );
                })()}
            </div>
        </div>
    ) : (
        <EmptyState
            icon={<MCP className="h-8 w-8 text-muted-foreground" />}
            title="选择一个MCP服务器"
            description="从左侧列表中选择一个服务器开始配置"
        />
    ), [selectedServer, serverTools, serverPrompts, serverResources, expandedTools, isRefreshing, handleToggleServer, handleRefreshServerCapabilities, openEditServerDialog, handleDeleteServer, toggleToolExpansion, handleUpdateTool, handleUpdatePrompt, truncateText]);

    // 空状态
    if (mcpServers.length === 0) {
        return (
            <>
                <ConfigPageLayout
                    sidebar={null}
                    content={null}
                    emptyState={
                        <EmptyState
                            icon={<MCP className="h-8 w-8 text-muted-foreground" />}
                            title="还没有配置MCP服务器"
                            description="开始添加你的第一个MCP服务器，扩展AI助手的能力"
                            action={
                                <MCPActionDropdown
                                    onTemplateSelect={handleTemplateSelect}
                                    onJSONImport={handleJSONImport}
                                    className="bg-primary hover:bg-primary/90 text-primary-foreground shadow-lg hover:shadow-xl transition-all"
                                />
                            }
                        />
                    }
                    showEmptyState={true}
                />

                {/* MCP服务器对话框 - 空状态时也需要渲染 */}
                <MCPServerDialog
                    isOpen={serverDialogOpen}
                    onClose={closeServerDialog}
                    onSubmit={handleServerDialogSubmit}
                    editingServer={editingServer}
                    initialServerType={dialogInitialServerType}
                    initialConfig={dialogInitialConfig}
                />

                {/* JSON导入对话框 */}
                <JSONImportDialog
                    isOpen={jsonImportDialogOpen}
                    onClose={closeJSONImportDialog}
                    onImport={handleJSONImportConfirm}
                />

                {/* 内置工具对话框 - 空状态时也需要渲染 */}
                <BuiltinToolDialog
                    isOpen={builtinDialogOpen}
                    onClose={() => setBuiltinDialogOpen(false)}
                    onSubmit={() => {
                        setBuiltinDialogOpen(false);
                        getMcpServers();
                    }}
                />
            </>
        );
    }

    return (
        <>
            <ConfigPageLayout
                sidebar={sidebar}
                content={content}
                selectOptions={selectOptions}
                selectedOptionId={selectedServer?.id.toString()}
                onSelectOption={handleSelectFromDropdown}
                selectPlaceholder="选择MCP服务器"
                addButton={addButton}
            />

            {/* MCP服务器对话框 */}
            <MCPServerDialog
                isOpen={serverDialogOpen}
                onClose={closeServerDialog}
                onSubmit={handleServerDialogSubmit}
                editingServer={editingServer}
                initialServerType={dialogInitialServerType}
                initialConfig={dialogInitialConfig}
            />

            {/* JSON导入对话框 */}
            <JSONImportDialog
                isOpen={jsonImportDialogOpen}
                onClose={closeJSONImportDialog}
                onImport={handleJSONImportConfirm}
            />

            {/* 内置工具对话框 */}
            <BuiltinToolDialog
                isOpen={builtinDialogOpen}
                onClose={() => setBuiltinDialogOpen(false)}
                onSubmit={() => {
                    setBuiltinDialogOpen(false);
                    getMcpServers();
                }}
            />

            {/* 内置工具编辑对话框 */}
            {editingServer && (
                <BuiltinToolDialog
                    isOpen={builtinEditOpen}
                    editing
                    initialName={editingServer.name}
                    initialDescription={editingServer.description || ''}
                    initialCommand={editingServer.command || ''}
                    initialEnvText={builtinEditEnv}
                    onEnvChange={setBuiltinEditEnv}
                    onNameChange={setBuiltinEditName}
                    onClose={() => setBuiltinEditOpen(false)}
                    onSubmit={async () => {
                        // Save name and env changes via update API
                        try {
                            const req: MCPServerRequest = {
                                name: builtinEditName || editingServer.name,
                                description: editingServer.description || undefined,
                                transport_type: editingServer.transport_type,
                                command: editingServer.command || undefined,
                                environment_variables: builtinEditEnv,
                                url: editingServer.url || undefined,
                                timeout: editingServer.timeout || undefined,
                                is_long_running: editingServer.is_long_running,
                                is_enabled: editingServer.is_enabled,
                                is_builtin: editingServer.is_builtin,
                            };
                            await invoke('update_mcp_server', { id: editingServer.id, request: req });

                            // 立即更新 selectedServer 以确保下次编辑时显示最新数据
                            if (selectedServer && selectedServer.id === editingServer.id) {
                                setSelectedServer({
                                    ...selectedServer,
                                    name: builtinEditName || editingServer.name,
                                    environment_variables: builtinEditEnv,
                                });
                            }

                            toast.success('已保存内置工具配置');
                            setBuiltinEditOpen(false);
                            getMcpServers();
                        } catch (e) {
                            toast.error('保存失败: ' + e);
                        }
                    }}
                />
            )}

            {/* 确认删除对话框 */}
            <ConfirmDialog
                isOpen={confirmDialogOpen}
                title="确认删除"
                confirmText="确定要删除这个MCP服务器吗？删除后相关配置将无法恢复。"
                onConfirm={confirmDeleteServer}
                onCancel={() => setConfirmDialogOpen(false)}
            />

            {/* 关闭 Agent MCP 确认对话框 */}
            <ConfirmDialog
                isOpen={disableOperationMcpConfirmOpen}
                title="确认关闭 Agent 工具集"
                confirmText={
                    disableOperationMcpCheckResult
                        ? `关闭该工具集将同时禁用以下助手的Skills：\n${
                            disableOperationMcpCheckResult.affected_assistants
                                .map(a => `• ${a.assistant_name}（${a.enabled_skills_count}个Skills）`)
                                .join('\n')
                          }\n\n确定要继续吗？`
                        : "关闭 Agent 工具集将同时禁用相关的Skills。确定要继续吗？"
                }
                onConfirm={handleConfirmDisableOperationMcp}
                onCancel={() => {
                    setDisableOperationMcpConfirmOpen(false);
                    setDisableOperationMcpCheckResult(null);
                    setPendingServerToggle(null);
                }}
            />
        </>
    );
};

export default MCPConfig;
