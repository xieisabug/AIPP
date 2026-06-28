import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";
import FetchUrlToolCall from "@/components/mcp-tool-components/FetchUrlToolCall";
import LoadMcpCatalogToolCall from "@/components/mcp-tool-components/LoadMcpCatalogToolCall";
import SearchWebToolCall from "@/components/mcp-tool-components/SearchWebToolCall";
import {
    mcpToolComponentRegistry,
    type McpToolComponentProps,
} from "@/services/mcpToolComponentRegistry";
import { parsePreviewCodeStreamingState } from "@/utils/previewCode";

let registered = false;

export function ensureBuiltinMcpToolComponentsRegistered(): void {
    if (registered) {
        return;
    }
    registered = true;

    mcpToolComponentRegistry.register("builtin", {
        id: "builtin.preview-code",
        label: "preview_code 预览组件",
        description: "用于 preview_code 工具的内联代码预览卡片",
        match: [{ toolName: "preview_code" }],
        priority: 100,
        shouldRender: (props: McpToolComponentProps) => props.status !== "failed",
        render: (props: McpToolComponentProps) => (
            <InlineCodePreviewCard
                parameters={props.parameters ?? "{}"}
                llmCallId={props.llmCallId}
                conversationId={props.conversationId}
                messageId={props.messageId}
                callId={props.callId}
                mcpToolCallStates={props.mcpToolCallStates}
                isStreaming={props.isStreaming}
                streamingPreviewState={parsePreviewCodeStreamingState(props.streamingPreviewState) ?? undefined}
                isLastMessage={props.isLastMessage}
            />
        ),
    });

    mcpToolComponentRegistry.register("builtin", {
        id: "builtin.search-web",
        label: "search_web 搜索组件",
        description: "用于 search_web 工具的紧凑搜索摘要卡片",
        match: [{ toolName: "search_web" }],
        priority: 110,
        render: (props: McpToolComponentProps) => <SearchWebToolCall {...props} />,
    });

    mcpToolComponentRegistry.register("builtin", {
        id: "builtin.fetch-url",
        label: "fetch_url 抓取组件",
        description: "用于 fetch_url 工具的紧凑网页抓取摘要卡片",
        match: [{ toolName: "fetch_url" }],
        priority: 110,
        render: (props: McpToolComponentProps) => <FetchUrlToolCall {...props} />,
    });

    mcpToolComponentRegistry.register("builtin", {
        id: "builtin.load-mcp-server",
        label: "load_mcp_server 加载组件",
        description: "用于 load_mcp_server 工具的轻量加载卡片",
        match: [{ toolName: "load_mcp_server" }],
        priority: 110,
        render: (props: McpToolComponentProps) => (
            <LoadMcpCatalogToolCall {...props} kind="server" />
        ),
    });

    mcpToolComponentRegistry.register("builtin", {
        id: "builtin.load-mcp-tool",
        label: "load_mcp_tool 加载组件",
        description: "用于 load_mcp_tool 工具的轻量加载卡片",
        match: [{ toolName: "load_mcp_tool" }],
        priority: 110,
        render: (props: McpToolComponentProps) => (
            <LoadMcpCatalogToolCall {...props} kind="tool" />
        ),
    });
}
