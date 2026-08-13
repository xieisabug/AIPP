import React from "react";
import McpToolCall from "@/components/McpToolCall";
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

const McpToolCallRenderer: React.FC<McpToolComponentProps> = (props) => {
    useMcpToolComponentRegistrySnapshot();
    const { config } = useDisplayConfig();
    const selectedComponentId = config?.mcp_tool_call_component_id || AUTO_MCP_TOOL_COMPONENT_ID;
    const enhancedProps = {
        ...props,
        currentToolCall: resolveCurrentToolCall(props),
    };
    const resolvedComponent = mcpToolComponentRegistry.resolve(enhancedProps, selectedComponentId);
    const fallback = renderDefaultMcpToolCall(enhancedProps);

    if (!resolvedComponent) {
        return fallback;
    }

    return (
        <McpToolComponentErrorBoundary
            key={resolvedComponent.id}
            fallback={fallback}
        >
            {resolvedComponent.render(enhancedProps)}
        </McpToolComponentErrorBoundary>
    );
};

export default McpToolCallRenderer;
