import React, { useContext } from "react";
import { ButlerResultsOnlyContext } from "./butler/ButlerPresentationContext";
import McpToolCall from "@/components/McpToolCall";
import { ToolReviewNotice } from "@/components/mcp-tool-components/ToolReviewNotice";
import { useToolReview } from "@/hooks/toolReviewStore";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";
import { ensureBuiltinMcpToolComponentsRegistered } from "@/services/builtinMcpToolComponents";
import {
    AUTO_MCP_TOOL_COMPONENT_ID,
    mcpToolComponentRegistry,
    useMcpToolComponentRegistrySnapshot,
    type McpToolComponentProps,
    type McpToolComponentToolCall,
} from "@/services/mcpToolComponentRegistry";

ensureBuiltinMcpToolComponentsRegistered();

class McpToolComponentErrorBoundary extends React.Component<
    { fallback: React.ReactNode; children: React.ReactNode },
    { hasError: boolean }
> {
    state = { hasError: false };

    static getDerivedStateFromError() {
        return { hasError: true };
    }

    componentDidCatch(error: unknown) {
        console.warn("[McpToolCallRenderer] MCP tool component failed, falling back to default", error);
    }

    render() {
        if (this.state.hasError) {
            return this.props.fallback;
        }
        return this.props.children;
    }
}

const resolveCurrentToolCall = (props: McpToolComponentProps): McpToolComponentToolCall | undefined => {
    if (props.currentToolCall) {
        return props.currentToolCall;
    }

    const byCallId = props.callId && props.mcpToolCallStates
        ? props.mcpToolCallStates.get(props.callId)
        : undefined;
    if (byCallId) {
        return { ...byCallId, id: byCallId.id ?? byCallId.call_id };
    }

    if (props.llmCallId && props.mcpToolCallStates) {
        for (const state of props.mcpToolCallStates.values()) {
            if (state.llm_call_id === props.llmCallId) {
                return { ...state, id: state.id ?? state.call_id };
            }
        }
    }

    if (!props.callId || !props.conversationId) {
        return undefined;
    }

    return {
        id: props.callId,
        call_id: props.callId,
        conversation_id: props.conversationId,
        message_id: props.messageId,
        status: props.status ?? "unknown",
        llm_call_id: props.llmCallId,
        server_name: props.serverName,
        tool_name: props.toolName,
        parameters: props.parameters,
        error: props.error,
    };
};

const renderDefaultMcpToolCall = (props: McpToolComponentProps) => (
    <McpToolCall
        serverName={props.serverName}
        toolName={props.toolName}
        parameters={props.parameters}
        llmCallId={props.llmCallId}
        status={props.status}
        error={props.error}
        conversationId={props.conversationId}
        messageId={props.messageId}
        callId={props.callId}
        mcpToolCallStates={props.mcpToolCallStates}
        shiningMcpCallId={props.shiningMcpCallId}
        isLastCall={props.isLastCall}
        isStreaming={props.isStreaming}
    />
);

/** 展示的是交给用户的结果（不是执行过程）的工具：管家简洁聊天里也要显示。 */
const RESULT_TOOL_NAMES = new Set(["preview_message"]);

const McpToolCallRenderer: React.FC<McpToolComponentProps> = (props) => {
    // Gate before resolving registry components: custom and preview renderers can show
    // transient execution UI even when their message is already marked complete.
    const resultsOnly = useContext(ButlerResultsOnlyContext);
    if (resultsOnly && !RESULT_TOOL_NAMES.has(props.toolName ?? "")) return null;
    return <ExecutionToolCall {...props} />;
};

const ExecutionToolCall: React.FC<McpToolComponentProps> = (props) => {
    useMcpToolComponentRegistrySnapshot();
    const { config } = useDisplayConfig();
    // 结果类工具的卡片就是内容本身，不受“统一使用默认工具卡片”的显示设置影响。
    const selectedComponentId = RESULT_TOOL_NAMES.has(props.toolName ?? "")
        ? AUTO_MCP_TOOL_COMPONENT_ID
        : config?.mcp_tool_call_component_id || AUTO_MCP_TOOL_COMPONENT_ID;
    const enhancedProps = {
        ...props,
        currentToolCall: resolveCurrentToolCall(props),
    };
    const effectiveCallId = enhancedProps.currentToolCall?.call_id
        ?? enhancedProps.currentToolCall?.id ?? enhancedProps.callId;
    const review = useToolReview(effectiveCallId);
    const currentStatus = enhancedProps.currentToolCall?.status ?? props.status;
    const resolvedComponent = mcpToolComponentRegistry.resolve(enhancedProps, selectedComponentId);
    const fallback = renderDefaultMcpToolCall(enhancedProps);

    // Specialized cards do not implement the review confirmation controls.
    if (!resolvedComponent || (currentStatus === "pending" && review)) {
        return fallback;
    }

    return (
        <div className="space-y-2">
            <ToolReviewNotice callId={enhancedProps.callId} />
            <McpToolComponentErrorBoundary
                key={resolvedComponent.id}
                fallback={fallback}
            >
                {resolvedComponent.render(enhancedProps)}
            </McpToolComponentErrorBoundary>
        </div>
    );
};

export default McpToolCallRenderer;
