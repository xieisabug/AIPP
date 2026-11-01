import React from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { Switch } from "../ui/switch";
import {
    Tooltip,
    TooltipContent,
    TooltipTrigger,
    TooltipProvider,
} from "../ui/tooltip";
import { MCPServerTool } from "../../data/MCP";
import MCPToolParameters from "./MCPToolParameters";
import { MOBILE_UNEXECUTABLE_LABEL } from "../../lib/mobileUnsupported";

interface MCPToolItemProps {
    tool: MCPServerTool;
    isExpanded: boolean;
    onToggleExpansion: (toolId: number) => void;
    onUpdateTool: (
        toolId: number,
        isEnabled: boolean,
        isAutoRun: boolean,
    ) => void;
    truncateText: (text: string, maxLines?: number) => string;
    /** 移动平台上该工具不可执行（仅追加标记，不隐藏配置） */
    mobileUnexecutable?: boolean;
}

const MCPToolItem: React.FC<MCPToolItemProps> = ({
    tool,
    isExpanded,
    onToggleExpansion,
    onUpdateTool,
    truncateText,
    mobileUnexecutable = false,
}) => {
    const hasParameters =
        tool.parameters &&
        tool.parameters !== "{}" &&
        tool.parameters !== "null";
    const parameters = hasParameters ? JSON.parse(tool.parameters!) : null;
    const truncatedDescription = truncateText(tool.tool_description || "", 2);
    const isDescriptionTruncated =
        tool.tool_description && truncatedDescription !== tool.tool_description;

    return (
        <div className="bg-muted rounded-lg overflow-hidden" data-aipp-slot="mcp-tool-item">
            {/* 纵向卡头：名称一行、说明全宽、操作可换行（窄屏 ~360px） */}
            <div className="flex flex-col gap-3 p-4">
                <div className="flex items-center gap-2 min-w-0">
                    <div className="font-medium text-foreground truncate min-w-0 flex-1" title={tool.tool_name}>
                        {tool.tool_name}
                    </div>
                    {hasParameters && (
                        <button
                            type="button"
                            onClick={() => onToggleExpansion(tool.id)}
                            className="flex-shrink-0 min-h-11 min-w-11 inline-flex items-center justify-center p-1 hover:bg-muted-foreground/20 rounded transition-colors"
                            title={isExpanded ? "收起参数" : "展开参数"}
                        >
                            {isExpanded ? (
                                <ChevronDown className="h-4 w-4 text-muted-foreground" />
                            ) : (
                                <ChevronRight className="h-4 w-4 text-muted-foreground" />
                            )}
                        </button>
                    )}
                </div>
                {mobileUnexecutable && (
                    <div className="-mt-2 text-[10px] text-muted-foreground" data-aipp-slot="mcp-tool-mobile-unexecutable">
                        {MOBILE_UNEXECUTABLE_LABEL}
                    </div>
                )}
                {tool.tool_description && (
                    <TooltipProvider delayDuration={1500}>
                        <Tooltip>
                            <TooltipTrigger asChild>
                                <div className="w-full text-sm text-muted-foreground leading-relaxed break-words">
                                    {isDescriptionTruncated
                                        ? truncatedDescription
                                        : tool.tool_description}
                                </div>
                            </TooltipTrigger>
                            {isDescriptionTruncated && (
                                <TooltipContent
                                    side="bottom"
                                    className="max-w-sm"
                                >
                                    <p className="text-sm">
                                        {tool.tool_description}
                                    </p>
                                </TooltipContent>
                            )}
                        </Tooltip>
                    </TooltipProvider>
                )}
                <div className="flex flex-wrap items-center gap-x-4 gap-y-2">
                    <div className="flex min-w-0 flex-1 items-center justify-between gap-2 sm:flex-initial">
                        <span className="text-sm text-foreground whitespace-nowrap">
                            启用
                        </span>
                        <Switch
                            className="shrink-0"
                            checked={tool.is_enabled}
                            onCheckedChange={(checked) =>
                                onUpdateTool(tool.id, checked, tool.is_auto_run)
                            }
                        />
                    </div>
                    <div className="flex min-w-0 flex-1 items-center justify-between gap-2 sm:flex-initial">
                        <span className="text-sm text-foreground whitespace-nowrap">
                            自动运行
                        </span>
                        <Switch
                            className="shrink-0"
                            checked={tool.is_auto_run}
                            onCheckedChange={(checked) =>
                                onUpdateTool(tool.id, tool.is_enabled, checked)
                            }
                        />
                    </div>
                </div>
            </div>

            {hasParameters && (
                <MCPToolParameters
                    isExpanded={isExpanded}
                    parameters={parameters}
                    truncateText={truncateText}
                />
            )}
        </div>
    );
};

export default MCPToolItem;
