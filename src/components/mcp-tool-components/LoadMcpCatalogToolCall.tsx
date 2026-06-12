import React, { useCallback, useEffect, useMemo, useState } from "react";
import { CheckCircle, Loader2, PackageSearch, Play, RotateCcw, Square, Wrench, XCircle } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ShineBorder } from "@/components/magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import { useToolErrorContinueEnabled } from "@/components/McpToolCall";
import { useAntiLeakage } from "@/contexts/AntiLeakageContext";
import { maskToolCall } from "@/utils/antiLeakage";
import { getErrorMessage } from "@/utils/error";
import type { MCPToolCall } from "@/data/MCPToolCall";
import type { McpToolComponentProps, McpToolCallStatus } from "@/services/mcpToolComponentRegistry";

type LoadMcpCatalogKind = "server" | "tool";

interface LoadMcpCatalogToolCallProps extends McpToolComponentProps {
    kind: LoadMcpCatalogKind;
}

const parseParameters = (parameters?: string): Record<string, unknown> => {
    if (!parameters) {
        return {};
    }
    try {
        const parsed = JSON.parse(parameters);
        return parsed && typeof parsed === "object" && !Array.isArray(parsed)
            ? parsed as Record<string, unknown>
            : {};
    } catch {
        return {};
    }
};

const stringValue = (value: unknown): string => {
    if (typeof value === "string") {
        return value.trim();
    }
    if (typeof value === "number" && Number.isFinite(value)) {
        return String(value);
    }
    return "";
};

const stringListValue = (value: unknown): string[] => {
    if (Array.isArray(value)) {
        return value
            .map((item) => stringValue(item))
            .filter(Boolean);
    }
    const single = stringValue(value);
    return single ? [single] : [];
};

const getEffectiveState = (
    props: LoadMcpCatalogToolCallProps,
): McpToolCallStatus | "streaming" | "idle" => {
    const stateOverride = props.callId && props.mcpToolCallStates
        ? props.mcpToolCallStates.get(props.callId)
        : undefined;
    if (props.isStreaming) {
        return "streaming";
    }
    return stateOverride?.status ?? props.status ?? "idle";
};

const statusLabel = (state: McpToolCallStatus | "streaming" | "idle"): string => {
    switch (state) {
        case "streaming":
            return "生成中";
        case "pending":
            return "待加载";
        case "executing":
            return "加载中";
        case "success":
            return "已加载";
        case "failed":
            return "加载失败";
        default:
            return "准备加载";
    }
};

const StatusBadge: React.FC<{ state: McpToolCallStatus | "streaming" | "idle" }> = ({ state }) => {
    if (state === "success") {
        return (
            <Badge
                variant="default"
                className="flex items-center gap-1 bg-success text-success-foreground border-success-border"
            >
                <CheckCircle className="h-3 w-3 text-success-foreground" />
                {statusLabel(state)}
            </Badge>
        );
    }
    if (state === "failed") {
        return (
            <Badge variant="destructive" className="flex items-center gap-1">
                <XCircle className="h-3 w-3" />
                {statusLabel(state)}
            </Badge>
        );
    }
    if (state === "executing" || state === "streaming") {
        return (
            <Badge variant="secondary" className="flex items-center gap-1">
                <Loader2 className="h-3 w-3 animate-spin" />
                {statusLabel(state)}
            </Badge>
        );
    }
    return <Badge variant="outline">{statusLabel(state)}</Badge>;
};

const TargetChip: React.FC<{ value: string }> = ({ value }) => (
    <span
        className="max-w-full truncate rounded-md border border-border bg-muted px-2 py-1 text-xs font-medium text-foreground"
        title={value}
    >
        {value}
    </span>
);

const LoadMcpCatalogToolCall: React.FC<LoadMcpCatalogToolCallProps> = (props) => {
    const matchedStateByLlmCallId = useMemo(() => {
        if (!props.mcpToolCallStates || !props.llmCallId || props.callId) {
            return undefined;
        }
        for (const state of props.mcpToolCallStates.values()) {
            if (state.llm_call_id === props.llmCallId) {
                return state;
            }
        }
        return undefined;
    }, [props.mcpToolCallStates, props.llmCallId, props.callId]);
    const [createdCallId, setCreatedCallId] = useState<number | null>(null);
    const effectiveCallId = props.callId ?? createdCallId ?? matchedStateByLlmCallId?.call_id ?? null;
    const parsedParameters = useMemo(
        () => parseParameters(props.parameters),
        [props.parameters],
    );
    const [localState, setLocalState] = useState<McpToolCallStatus | "streaming" | "idle">(
        getEffectiveState(props),
    );
    const [localError, setLocalError] = useState<string | null>(props.error ?? null);
    const stateOverride = effectiveCallId && props.mcpToolCallStates
        ? props.mcpToolCallStates.get(effectiveCallId)
        : matchedStateByLlmCallId;
    const state = props.isStreaming ? "streaming" : stateOverride?.status ?? localState;
    const isRunning = Boolean(effectiveCallId && props.shiningMcpCallId === effectiveCallId);
    const shouldShine = isRunning || props.isStreaming || state === "executing";
    const continueOnToolErrorEnabled = useToolErrorContinueEnabled();
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    const shouldMask = antiLeakageEnabled && !isRevealed;

    const serverName = stringValue(parsedParameters.server_name);
    const serverKeyword = stringValue(parsedParameters.name);
    const toolNames = stringListValue(parsedParameters.names ?? parsedParameters.name);

    const masked = shouldMask
        ? maskToolCall(props.serverName ?? "", props.toolName ?? "", props.parameters ?? "{}")
        : null;

    const displayServerName = shouldMask ? masked?.serverName ?? "******" : serverName;
    const displayServerKeyword = shouldMask ? masked?.parameters ?? "******" : serverKeyword;
    const displayToolNames = shouldMask ? ["******"] : toolNames;

    const isServerLoader = props.kind === "server";
    const Icon = isServerLoader ? PackageSearch : Wrench;
    const title = isServerLoader ? "装载工具集" : "加载工具";
    const isProtocolFailureWithoutCall = !effectiveCallId
        && state === "failed"
        && (props.status === "failed" || Boolean(props.error));
    const shouldHideFailedActions = state === "failed" && continueOnToolErrorEnabled;
    const canExecute = state === "idle" || state === "pending" || state === "failed";
    const isExecuting = state === "executing";
    const canShowExecute = canExecute
        && !props.isStreaming
        && !shouldHideFailedActions
        && !isProtocolFailureWithoutCall;
    const effectiveError = stateOverride?.error ?? localError ?? props.error ?? null;
    const displayError = effectiveError
        ? shouldMask
            ? "******"
            : effectiveError.includes("Stopped by user")
                ? "用户已停止"
                : effectiveError
        : null;

    useEffect(() => {
        if (props.isStreaming) {
            setLocalState("streaming");
            return;
        }
        if (stateOverride?.status) {
            setLocalState(stateOverride.status);
            setLocalError(stateOverride.error ?? null);
            return;
        }
        setLocalState(props.status ?? "idle");
        setLocalError(props.error ?? null);
    }, [props.isStreaming, props.status, props.error, stateOverride?.status, stateOverride?.error]);

    const handleExecute = useCallback(async () => {
        if (!props.conversationId) {
            setLocalState("failed");
            setLocalError("conversation_id is required for execution");
            return;
        }

        try {
            setLocalState("executing");
            setLocalError(null);
            let currentCallId = effectiveCallId;

            if (!currentCallId) {
                const createdCall = await invoke<MCPToolCall>("create_mcp_tool_call", {
                    conversationId: props.conversationId,
                    messageId: props.messageId,
                    serverName: props.serverName,
                    toolName: props.toolName,
                    parameters: props.parameters ?? "{}",
                });
                currentCallId = createdCall.id;
                setCreatedCallId(currentCallId);
            }

            const result = await invoke<MCPToolCall>("execute_mcp_tool_call", {
                callId: currentCallId,
                triggerContinuation: props.isLastCall,
            });
            setLocalState(result.status);
            setLocalError(result.error ?? null);
        } catch (error) {
            setLocalState("failed");
            setLocalError(getErrorMessage(error) || "执行失败");
        }
    }, [
        effectiveCallId,
        props.conversationId,
        props.messageId,
        props.serverName,
        props.toolName,
        props.parameters,
        props.isLastCall,
    ]);

    const handleStop = useCallback(async () => {
        if (!effectiveCallId) {
            return;
        }
        try {
            await invoke("stop_mcp_tool_call", { callId: effectiveCallId });
        } catch (error) {
            setLocalState("failed");
            setLocalError(getErrorMessage(error) || "停止失败");
        }
    }, [effectiveCallId]);

    return (
        <div className="w-full max-w-[600px] my-1 p-2 border border-border rounded-md bg-card overflow-hidden relative">
            {shouldShine && (
                <ShineBorder
                    shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                    borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                    duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                />
            )}
            <div className="space-y-2">
                <div className="flex items-start justify-between gap-3">
                    <div className="flex min-w-0 flex-1 items-start gap-2">
                        <Icon className="mt-0.5 h-4 w-4 flex-shrink-0 text-muted-foreground" />
                        <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-medium text-foreground" title={title}>
                                {title}
                            </div>
                        </div>
                    </div>
                    <div className="flex flex-shrink-0 items-center gap-1">
                        <div title={displayError ?? undefined}>
                            <StatusBadge state={state} />
                        </div>
                        {isExecuting && effectiveCallId && (
                            <Button
                                onClick={handleStop}
                                size="sm"
                                variant="ghost"
                                className="h-7 w-7 p-0 flex-shrink-0 text-destructive"
                                title="停止"
                            >
                                <Square className="h-3 w-3 fill-current" />
                            </Button>
                        )}
                        {canShowExecute && (
                            <Button
                                onClick={handleExecute}
                                size="sm"
                                variant="ghost"
                                className="h-7 w-7 p-0 flex-shrink-0"
                                title={state === "failed" ? "重新执行" : "执行"}
                            >
                                {state === "failed" ? (
                                    <RotateCcw className="h-3 w-3" />
                                ) : (
                                    <Play className="h-3 w-3" />
                                )}
                            </Button>
                        )}
                    </div>
                </div>
                <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    {isServerLoader ? (
                        <TargetChip value={displayServerKeyword || "未指定关键词"} />
                    ) : (
                        <>
                            {displayServerName && (
                                <TargetChip value={`工具集: ${displayServerName}`} />
                            )}
                            {displayToolNames.length > 0 ? (
                                displayToolNames.map((name, index) => (
                                    <TargetChip key={`${name}-${index}`} value={name} />
                                ))
                            ) : (
                                <TargetChip value="未指定工具" />
                            )}
                        </>
                    )}
                </div>
                {state === "failed" && displayError && (
                    <div
                        className="max-h-24 overflow-auto whitespace-pre-wrap break-words rounded-md border border-destructive/20 bg-destructive/5 px-2 py-1.5 text-xs text-destructive"
                        title={displayError}
                    >
                        错误原因：{displayError}
                    </div>
                )}
            </div>
        </div>
    );
};

export default LoadMcpCatalogToolCall;
