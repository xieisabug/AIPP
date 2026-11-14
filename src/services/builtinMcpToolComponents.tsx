import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";
import FetchUrlToolCall from "@/components/mcp-tool-components/FetchUrlToolCall";
import LoadMcpCatalogToolCall from "@/components/mcp-tool-components/LoadMcpCatalogToolCall";
import MessagePreviewToolCall from "@/components/mcp-tool-components/MessagePreviewToolCall";
import SearchWebToolCall from "@/components/mcp-tool-components/SearchWebToolCall";
import VaultRequestToolCall, { VAULT_CARD_STATUSES } from "@/components/mcp-tool-components/VaultRequestToolCall";
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
        id: "builtin.preview-message",
        label: "preview_message 引用卡片",
        description: "用于 preview_message 工具：在当前会话里原样展示另一条助手回复",
        match: [{ toolName: "preview_message" }],
        priority: 110,
        render: (props: McpToolComponentProps) => <MessagePreviewToolCall {...props} />,
    });

    mcpToolComponentRegistry.register("builtin", {
        id: "builtin.vault-request",
        label: "vault_http_request 凭据请求卡片",
        description: "用于保险箱凭据请求：一行显示访问了哪个网站，展开查看响应",
        match: [{ toolName: "vault_http_request" }],
        priority: 110,
        shouldRender: (props: McpToolComponentProps) =>
            VAULT_CARD_STATUSES.has(props.currentToolCall?.status ?? props.status ?? ""),
        render: (props: McpToolComponentProps) => <VaultRequestToolCall {...props} />,
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
