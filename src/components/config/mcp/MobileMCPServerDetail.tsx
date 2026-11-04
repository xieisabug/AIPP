import React from "react";
import { Edit, MoreHorizontal, RefreshCw, Trash2 } from "lucide-react";
import MCP from "@/assets/mcp.svg?react";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { MCPServer, MCPServerPrompt, MCPServerResource, MCPServerTool } from "@/data/MCP";
import { MOBILE_UNEXECUTABLE_LABEL } from "@/lib/mobileUnsupported";
import MCPToolParameters from "../MCPToolParameters";

interface MobileMCPServerDetailProps {
    server: MCPServer;
    tools: MCPServerTool[];
    prompts: MCPServerPrompt[];
    resources: MCPServerResource[];
    isRefreshing: boolean;
    /** 外部 stdio 等在本设备不可执行（仅提示，不隐藏配置）。 */
    serverUnexecutable: boolean;
    isToolUnexecutable: (toolName: string) => boolean;
    expandedTools: Set<number>;
    onToggleToolExpansion: (toolId: number) => void;
    onToggleServer: (serverId: number, isEnabled: boolean) => void;
    onRefresh: (serverId: number) => void;
    onEdit: (server: MCPServer) => void;
    onDelete: (serverId: number) => void;
    onUpdateTool: (toolId: number, isEnabled: boolean, isAutoRun: boolean) => void;
    onUpdatePrompt: (promptId: number, isEnabled: boolean) => void;
    truncateText: (text: string, maxLines?: number) => string;
}

const InfoRow: React.FC<{ label: string; children: React.ReactNode }> = ({ label, children }) => (
    <div className="flex min-h-11 items-center justify-between gap-3 py-2">
        <span className="text-sm text-foreground">{label}</span>
        <span className="min-w-0 truncate text-right text-sm text-muted-foreground">{children}</span>
    </div>
);

const parseParameters = (raw?: string | null) => {
    if (!raw || raw === "{}" || raw === "null") return null;
    try {
        const parsed = JSON.parse(raw);
        return parsed?.properties && Object.keys(parsed.properties).length > 0 ? parsed : null;
    } catch {
        return null;
    }
};

/** 移动端 MCP 详情：单列分组行 + 紧凑的工具行，替代桌面的双层卡片与并排按钮组。 */
const MobileMCPServerDetail: React.FC<MobileMCPServerDetailProps> = ({
    server,
    tools,
    prompts,
    resources,
    isRefreshing,
    serverUnexecutable,
    isToolUnexecutable,
    expandedTools,
    onToggleToolExpansion,
    onToggleServer,
    onRefresh,
    onEdit,
    onDelete,
    onUpdateTool,
    onUpdatePrompt,
    truncateText,
}) => {
    const tabs = [
        tools.length > 0 && { value: "tools", label: `工具 ${tools.length}` },
        prompts.length > 0 && { value: "prompts", label: `提示 ${prompts.length}` },
        resources.length > 0 && { value: "resources", label: `资源 ${resources.length}` },
    ].filter(Boolean) as { value: string; label: string }[];
    const enabledToolCount = tools.filter(tool => tool.is_enabled).length;

    return (
        <div className="space-y-5 pb-4" data-aipp-slot="mcp-mobile-detail">
            <section className="flex items-start gap-2">
                <div className="min-w-0 flex-1">
                    <h3 className="break-words text-base font-semibold text-foreground">{server.name}</h3>
                    <p className="mt-1 break-words text-sm text-muted-foreground">{server.description || "暂无描述"}</p>
                </div>
                <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                        <Button type="button" variant="ghost" size="icon" aria-label="更多操作" className="-mr-2 flex-shrink-0">
                            <MoreHorizontal className="h-5 w-5" />
                        </Button>
                    </DropdownMenuTrigger>
                    <DropdownMenuContent align="end">
                        <DropdownMenuItem onSelect={() => onEdit(server)}>
                            <Edit className="h-4 w-4" />
                            编辑
                        </DropdownMenuItem>
                        {server.is_deletable && (
                            <DropdownMenuItem variant="destructive" onSelect={() => onDelete(server.id)}>
                                <Trash2 className="h-4 w-4" />
                                删除
                            </DropdownMenuItem>
                        )}
                    </DropdownMenuContent>
                </DropdownMenu>
            </section>

            {serverUnexecutable && (
                <p className="rounded-md bg-muted px-3 py-2 text-xs leading-5 text-muted-foreground">
                    此 MCP 依赖外部进程，移动端不可执行（可查看配置；HTTP 与 aipp:* 内置仍可用）。
                </p>
            )}

            <section className="divide-y divide-border border-y border-border" data-aipp-slot="mcp-mobile-info">
                <div className="flex min-h-12 items-center justify-between gap-3 py-2">
                    <span className="text-sm font-medium text-foreground">启用此 MCP</span>
                    <Switch
                        checked={server.is_enabled}
                        aria-label="启用此 MCP"
                        onCheckedChange={(checked) => onToggleServer(server.id, checked)}
                    />
                </div>
                <InfoRow label="传输类型">{server.transport_type}</InfoRow>
                <InfoRow label="长期运行">{server.is_long_running ? "是" : "否"}</InfoRow>
                {server.timeout ? <InfoRow label="超时时间">{server.timeout}ms</InfoRow> : null}
                {tools.length > 0 && <InfoRow label="已启用工具">{enabledToolCount} / {tools.length}</InfoRow>}
            </section>

            <Button
                type="button"
                variant="outline"
                className="w-full gap-2"
                disabled={isRefreshing}
                onClick={() => onRefresh(server.id)}
            >
                <RefreshCw className={`h-4 w-4 ${isRefreshing ? "animate-spin" : ""}`} />
                {isRefreshing ? "正在连接…" : "测试连接并刷新能力"}
            </Button>

            {tabs.length > 0 ? (
                <Tabs defaultValue={tabs[0].value} className="w-full">
                    <TabsList className="w-full">
                        {tabs.map(tab => (
                            <TabsTrigger key={tab.value} value={tab.value} className="flex-1">{tab.label}</TabsTrigger>
                        ))}
                    </TabsList>

                    {tools.length > 0 && (
                        <TabsContent value="tools" className="mt-2">
                            <ul className="divide-y divide-border">
                                {tools.map((tool) => {
                                    const parameters = parseParameters(tool.parameters);
                                    const expanded = expandedTools.has(tool.id);
                                    const canExpand = Boolean(parameters) || (tool.tool_description?.length ?? 0) > 60;
                                    return (
                                        <li key={tool.id} className="py-3" data-aipp-slot="mcp-mobile-tool">
                                            <div className="flex items-start gap-3">
                                                <div className="min-w-0 flex-1">
                                                    <div className="break-all text-sm font-medium text-foreground">{tool.tool_name}</div>
                                                    {isToolUnexecutable(tool.tool_name) && (
                                                        <div className="mt-0.5 text-[11px] text-muted-foreground">{MOBILE_UNEXECUTABLE_LABEL}</div>
                                                    )}
                                                    {tool.tool_description && (
                                                        <p className={`mt-1 break-words text-xs leading-5 text-muted-foreground ${expanded ? "" : "line-clamp-2"}`}>
                                                            {tool.tool_description}
                                                        </p>
                                                    )}
                                                </div>
                                                <Switch
                                                    className="mt-0.5 flex-shrink-0"
                                                    checked={tool.is_enabled}
                                                    aria-label={`启用 ${tool.tool_name}`}
                                                    onCheckedChange={(checked) => onUpdateTool(tool.id, checked, tool.is_auto_run)}
                                                />
                                            </div>
                                            <div className="mt-1 flex items-center justify-between gap-3">
                                                <div className="flex min-h-11 items-center gap-2">
                                                    <Switch
                                                        checked={tool.is_auto_run}
                                                        aria-label={`${tool.tool_name} 自动运行`}
                                                        onCheckedChange={(checked) => onUpdateTool(tool.id, tool.is_enabled, checked)}
                                                    />
                                                    <span className="text-xs text-muted-foreground">自动运行</span>
                                                </div>
                                                {canExpand && (
                                                    <button
                                                        type="button"
                                                        className="min-h-11 px-2 text-xs text-muted-foreground"
                                                        aria-expanded={expanded}
                                                        onClick={() => onToggleToolExpansion(tool.id)}
                                                    >
                                                        {expanded ? "收起" : parameters ? "参数与说明" : "展开说明"}
                                                    </button>
                                                )}
                                            </div>
                                            {parameters && expanded && (
                                                <div className="mt-1 rounded-md bg-muted/50 pt-3">
                                                    <MCPToolParameters isExpanded parameters={parameters} truncateText={truncateText} />
                                                </div>
                                            )}
                                        </li>
                                    );
                                })}
                            </ul>
                        </TabsContent>
                    )}

                    {prompts.length > 0 && (
                        <TabsContent value="prompts" className="mt-2">
                            <ul className="divide-y divide-border">
                                {prompts.map((prompt) => (
                                    <li key={prompt.id} className="flex items-start gap-3 py-3">
                                        <div className="min-w-0 flex-1">
                                            <div className="break-all text-sm font-medium text-foreground">{prompt.prompt_name}</div>
                                            {prompt.prompt_description && (
                                                <p className="mt-1 break-words text-xs leading-5 text-muted-foreground">{prompt.prompt_description}</p>
                                            )}
                                        </div>
                                        <Switch
                                            className="mt-0.5 flex-shrink-0"
                                            checked={prompt.is_enabled}
                                            aria-label={`启用 ${prompt.prompt_name}`}
                                            onCheckedChange={(checked) => onUpdatePrompt(prompt.id, checked)}
                                        />
                                    </li>
                                ))}
                            </ul>
                        </TabsContent>
                    )}

                    {resources.length > 0 && (
                        <TabsContent value="resources" className="mt-2">
                            <ul className="divide-y divide-border">
                                {resources.map((resource) => (
                                    <li key={resource.id} className="py-3">
                                        <div className="flex items-center gap-2">
                                            <span className="min-w-0 flex-1 break-all text-sm font-medium text-foreground">{resource.resource_name}</span>
                                            <span className="flex-shrink-0 rounded border border-border px-1.5 text-[11px] text-muted-foreground">{resource.resource_type}</span>
                                        </div>
                                        <div className="mt-1 break-all text-xs text-muted-foreground">{resource.resource_uri}</div>
                                        {resource.resource_description && (
                                            <p className="mt-1 break-words text-xs leading-5 text-muted-foreground">{resource.resource_description}</p>
                                        )}
                                    </li>
                                ))}
                            </ul>
                        </TabsContent>
                    )}
                </Tabs>
            ) : (
                <div className="py-8 text-center">
                    <MCP className="mx-auto mb-3 h-10 w-10 text-muted-foreground" />
                    <p className="text-sm text-muted-foreground">暂无能力数据</p>
                    <p className="mt-1 text-xs text-muted-foreground">点击「测试连接并刷新能力」获取服务器能力</p>
                </div>
            )}
        </div>
    );
};

export default MobileMCPServerDetail;
