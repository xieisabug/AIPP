import React, { useCallback, useEffect, useMemo, useState } from "react";
import {
    ArrowRight,
    CheckCircle,
    Clock,
    Globe,
    Hash,
    Loader2,
    Play,
    RotateCcw,
    Square,
    XCircle,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
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

interface ParsedFetchResult {
    fetchTimeMs?: number;
    wordCount?: number;
}

const parseParameters = (parameters?: string): Record<string, unknown> => {
    if (!parameters) {
        return {};
    }
    try {
        const parsed = JSON.parse(parameters);
        return parsed && typeof parsed === "object" && !Array.isArray(parsed)
            ? (parsed as Record<string, unknown>)
            : {};
    } catch {
        return {};
    }
};

const stringValue = (value: unknown): string => {
    if (typeof value === "string") {
        return value.trim();
    }
    return "";
};

const isRecord = (value: unknown): value is Record<string, unknown> => (
    Boolean(value) && typeof value === "object" && !Array.isArray(value)
);

const isToolContentPart = (value: unknown): value is Record<string, unknown> => (
    isRecord(value) && typeof value.type === "string" && ("text" in value || "json" in value)
);

const numberValue = (value: unknown): number | undefined => (
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : undefined
);

const timestampMs = (value?: Date | string): number | undefined => {
    if (!value) {
        return undefined;
    }
    const time = value instanceof Date ? value.getTime() : new Date(value).getTime();
    return Number.isFinite(time) ? time : undefined;
};

const elapsedMsFromToolCall = (started?: Date | string, finished?: Date | string): number | undefined => {
    const startedMs = timestampMs(started);
    const finishedMs = timestampMs(finished);
    if (typeof startedMs !== "number" || typeof finishedMs !== "number" || finishedMs < startedMs) {
        return undefined;
    }
    return finishedMs - startedMs;
};

const formatDurationSeconds = (durationMs: number): string => {
    const seconds = durationMs / 1000;
    if (seconds >= 10) {
        return `${Math.round(seconds)} 秒`;
    }
    return `${Number(seconds.toFixed(1))} 秒`;
};

const getToolContentParts = (parsed: unknown): Record<string, unknown>[] => {
    if (Array.isArray(parsed)) {
        return parsed.filter(isToolContentPart);
    }
    if (isRecord(parsed) && Array.isArray(parsed.content)) {
        return parsed.content.filter(isToolContentPart);
    }
    return [];
};

const extractTextFromResult = (result?: string): string => {
    if (!result) {
        return "";
    }
    try {
        const parsed = JSON.parse(result);
        if (!parsed || typeof parsed !== "object") {
            return result;
        }
        const content = getToolContentParts(parsed);
        const texts: string[] = [];
        for (const item of content) {
            if (typeof item.text === "string") {
                texts.push(item.text);
            }
        }
        if (texts.length > 0) {
            return texts.join("\n").trim();
        }
        return typeof parsed.text === "string" ? parsed.text.trim() : "";
    } catch {
        return result;
    }
};

const countWords = (text: string): number => {
    if (!text) {
        return 0;
    }
    // 匹配连续中文字符作为词，再匹配非空白西文词
    const cjkMatches = text.match(/[一-龥]/g) || [];
    const wordMatches = text.match(/[a-zA-Z0-9_]+/g) || [];
    return cjkMatches.length + wordMatches.length;
};

const applyStructuredFetchPayload = (payload: unknown, output: ParsedFetchResult): boolean => {
    if (!isRecord(payload)) {
        return false;
    }

    const fetchTimeMs = numberValue(payload.fetch_time_ms)
        ?? numberValue(payload.elapsed_ms)
        ?? numberValue(payload.duration_ms);
    if (typeof fetchTimeMs === "number") {
        output.fetchTimeMs = fetchTimeMs;
    }

    return typeof fetchTimeMs === "number";
};

const parseFetchResult = (result?: string, fallbackFetchTimeMs?: number): ParsedFetchResult => {
    const output: ParsedFetchResult = {
        fetchTimeMs: fallbackFetchTimeMs,
    };
    const text = extractTextFromResult(result);
    if (text) {
        output.wordCount = countWords(text);
    }

    if (result) {
        try {
            const parsed = JSON.parse(result);
            applyStructuredFetchPayload(parsed, output);
            for (const part of getToolContentParts(parsed)) {
                if (part.type === "json" && "json" in part) {
                    applyStructuredFetchPayload(part.json, output);
                }
            }
        } catch {
            // Raw text result only contributes word count.
        }
    }
    return output;
};

const getEffectiveState = (
    props: McpToolComponentProps,
): McpToolCallStatus | "streaming" | "idle" => {
    if (props.isStreaming) {
        return "streaming";
    }
    return props.currentToolCall?.status ?? props.status ?? "idle";
};

const statusLabel = (state: McpToolCallStatus | "streaming" | "idle"): string => {
    switch (state) {
        case "streaming":
            return "生成中";
        case "pending":
            return "待执行";
        case "executing":
            return "抓取中";
        case "success":
            return "抓取完成";
        case "failed":
            return "抓取失败";
        default:
            return "准备抓取";
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

const MetaItem: React.FC<{
    icon: React.ReactNode;
    value: string;
    title: string;
}> = ({ icon, value, title }) => (
    <span
        className="inline-flex items-center gap-1 text-xs text-muted-foreground"
        title={title}
    >
        <span className="flex-shrink-0">{icon}</span>
        <span className="font-medium text-foreground">{value}</span>
    </span>
);

const FetchUrlToolCall: React.FC<McpToolComponentProps> = (props) => {
    const parsedParameters = useMemo(() => parseParameters(props.parameters), [props.parameters]);
    const url = stringValue(parsedParameters.url) || "未指定 URL";

    const [localState, setLocalState] = useState<McpToolCallStatus | "streaming" | "idle">(
        getEffectiveState(props),
    );
    const [localError, setLocalError] = useState<string | null>(props.error ?? null);
    const [createdCallId, setCreatedCallId] = useState<number | null>(null);

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

    const effectiveCallId = props.callId ?? createdCallId ?? matchedStateByLlmCallId?.call_id ?? null;
    const stateOverride = effectiveCallId && props.mcpToolCallStates
        ? props.mcpToolCallStates.get(effectiveCallId)
        : matchedStateByLlmCallId;
    const state = props.isStreaming ? "streaming" : stateOverride?.status ?? localState;
    const effectiveResult = stateOverride?.result ?? props.currentToolCall?.result;
    const fallbackFetchTimeMs = useMemo(
        () => elapsedMsFromToolCall(
            stateOverride?.started_time ?? props.currentToolCall?.started_time,
            stateOverride?.finished_time ?? props.currentToolCall?.finished_time,
        ),
        [
            stateOverride?.started_time,
            stateOverride?.finished_time,
            props.currentToolCall?.started_time,
            props.currentToolCall?.finished_time,
        ],
    );
    const parsedResult = useMemo(
        () => parseFetchResult(effectiveResult, fallbackFetchTimeMs),
        [effectiveResult, fallbackFetchTimeMs],
    );

    const isRunning = Boolean(effectiveCallId && props.shiningMcpCallId === effectiveCallId);
    const shouldShine = isRunning || props.isStreaming || state === "executing";
    const isExecuting = state === "executing";
    const isFailed = state === "failed";
    const canExecute = state === "idle" || state === "pending" || state === "failed";
    const continueOnToolErrorEnabled = useToolErrorContinueEnabled();
    const isProtocolFailureWithoutCall = !effectiveCallId
        && isFailed
        && (props.status === "failed" || Boolean(props.error));
    const shouldHideFailedActions = isFailed && continueOnToolErrorEnabled;
    const canShowExecute = canExecute
        && !props.isStreaming
        && !shouldHideFailedActions
        && !isProtocolFailureWithoutCall;
    const canShowContinueWithError = isFailed
        && Boolean(effectiveCallId)
        && !props.isStreaming
        && !shouldHideFailedActions;
    const effectiveError = stateOverride?.error ?? localError ?? props.error ?? null;

    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    const shouldMask = antiLeakageEnabled && !isRevealed;
    const masked = shouldMask
        ? maskToolCall(props.serverName ?? "", props.toolName ?? "", props.parameters ?? "{}")
        : null;
    const displayUrl = shouldMask ? masked?.parameters ?? "******" : url;
    const displayError = shouldMask && effectiveError ? "******" : effectiveError;
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

    const handleExecute = useCallback(async (event?: React.MouseEvent) => {
        event?.stopPropagation();
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

    const handleStop = useCallback(async (event?: React.MouseEvent) => {
        event?.stopPropagation();
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

    const handleContinueWithError = useCallback(async (event?: React.MouseEvent) => {
        event?.stopPropagation();
        if (!effectiveCallId) {
            return;
        }
        try {
            await invoke("continue_with_error", {
                callId: effectiveCallId,
                errorMessage: effectiveError,
            });
        } catch (error) {
            setLocalError(getErrorMessage(error) || "继续失败");
        }
    }, [effectiveCallId, effectiveError]);

    const canFocusInSidebar = state === "success" && effectiveCallId !== null;

    const handleFocusInSidebar = useCallback(async () => {
        if (!effectiveCallId) {
            return;
        }
        try {
            await emit("sidebar-focus-context", { id: `mcp-${effectiveCallId}` });
        } catch (error) {
            console.warn("[FetchUrlToolCall] focus sidebar failed", error);
        }
    }, [effectiveCallId]);

    const handleCardClick = useCallback(() => {
        if (!canFocusInSidebar) {
            return;
        }
        void handleFocusInSidebar();
    }, [canFocusInSidebar, handleFocusInSidebar]);

    const handleCardKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
        if (!canFocusInSidebar) {
            return;
        }
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            void handleFocusInSidebar();
        }
    }, [canFocusInSidebar, handleFocusInSidebar]);

    return (
        <div
            className={`w-full max-w-[600px] my-1 p-2 border border-border rounded-md bg-card overflow-hidden relative transition-colors${canFocusInSidebar ? " cursor-pointer hover:border-primary/40 hover:bg-muted/30" : ""}`}
            onClick={handleCardClick}
            onKeyDown={handleCardKeyDown}
            role={canFocusInSidebar ? "button" : undefined}
            tabIndex={canFocusInSidebar ? 0 : undefined}
            aria-label={canFocusInSidebar ? "在侧栏中定位抓取详情" : undefined}
        >
            {shouldShine && (
                <ShineBorder
                    shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                    borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                    duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                />
            )}
            <div className="space-y-1.5">
                <div className="flex items-start justify-between gap-3">
                    <div className="flex min-w-0 flex-1 items-start gap-2">
                        <Globe className="mt-0.5 h-4 w-4 flex-shrink-0 text-muted-foreground" />
                        <div className="min-w-0 flex-1">
                            <div className="flex items-center gap-1 min-w-0">
                                <span
                                    className="min-w-0 truncate text-left text-sm font-medium text-foreground"
                                    title={displayUrl}
                                >
                                    {displayUrl}
                                </span>
                            </div>
                            <div className="text-xs text-muted-foreground mt-0.5">抓取网页</div>
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
                                title={isFailed ? "重新执行" : "执行"}
                            >
                                {isFailed ? (
                                    <RotateCcw className="h-3 w-3" />
                                ) : (
                                    <Play className="h-3 w-3" />
                                )}
                            </Button>
                        )}
                        {canShowContinueWithError && (
                            <Button
                                onClick={handleContinueWithError}
                                size="sm"
                                variant="ghost"
                                className="h-7 w-7 p-0 flex-shrink-0"
                                title="以错误继续对话"
                            >
                                <ArrowRight className="h-3 w-3" />
                            </Button>
                        )}
                    </div>
                </div>

                {state === "success" && (typeof parsedResult.wordCount === "number" || typeof parsedResult.fetchTimeMs === "number") && (
                    <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                        {typeof parsedResult.wordCount === "number" && (
                            <MetaItem
                                icon={<Hash className="h-3 w-3" />}
                                value={`${parsedResult.wordCount} 字`}
                                title={`参考文档：${parsedResult.wordCount} 字`}
                            />
                        )}
                        {typeof parsedResult.fetchTimeMs === "number" && (
                            <MetaItem
                                icon={<Clock className="h-3 w-3" />}
                                value={formatDurationSeconds(parsedResult.fetchTimeMs)}
                                title={`抓取耗时：${formatDurationSeconds(parsedResult.fetchTimeMs)}`}
                            />
                        )}
                    </div>
                )}

                {state === "failed" && displayError && (
                    <div
                        className="max-h-24 overflow-auto whitespace-pre-wrap break-words rounded-md border border-destructive/20 bg-destructive/5 px-2 py-1.5 text-xs text-destructive"
                        title={displayError}
                    >
                        {displayError}
                    </div>
                )}
            </div>
        </div>
    );
};

export default FetchUrlToolCall;
