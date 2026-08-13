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
    Search,
    Square,
    XCircle,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ShineBorder } from "@/components/magicui/shine-border";
import { MotionDetails, MotionMetaRow, MotionMetricItem, MotionStatusSlot, MotionToolCard } from "@/components/mcp-tool-components/McpToolMotion";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import { useToolErrorContinueEnabled } from "@/components/McpToolCall";
import { useAntiLeakage } from "@/contexts/AntiLeakageContext";
import { maskToolCall } from "@/utils/antiLeakage";
import { getErrorMessage } from "@/utils/error";
import type { MCPToolCall } from "@/data/MCPToolCall";
import type { McpToolComponentProps, McpToolCallStatus } from "@/services/mcpToolComponentRegistry";

interface ParsedSearchResult {
    engine?: string;
    resultCount?: number;
    searchTimeMs?: number;
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
    isRecord(value) && typeof value.type === "string" && ("json" in value || "text" in value)
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

// Estimate result count from SERP markdown by counting distinct external links.
// SERP pages have many non-result links (nav, pagination, etc.), but result
// entries are usually the bulk of distinct external URLs — deduping gives a
// reasonable approximation. Returns undefined when no links are found.
const estimateResultCountFromMarkdown = (markdown: string): number | undefined => {
    if (!markdown) {
        return undefined;
    }
    const linkPattern = /\[[^\]]*\]\((https?:\/\/[^\s)]+)\)/g;
    const urls = new Set<string>();
    let match: RegExpExecArray | null;
    while ((match = linkPattern.exec(markdown)) !== null) {
        urls.add(match[1]);
    }
    return urls.size > 0 ? urls.size : undefined;
};

const applyStructuredSearchPayload = (payload: unknown, output: ParsedSearchResult): boolean => {
    if (Array.isArray(payload)) {
        output.resultCount = payload.length;
        return true;
    }
    if (!isRecord(payload)) {
        return false;
    }

    if (Array.isArray(payload.items)) {
        output.resultCount = payload.items.length;
    }

    const searchTimeMs = numberValue(payload.search_time_ms);
    if (typeof searchTimeMs === "number") {
        output.searchTimeMs = searchTimeMs;
    }

    if (typeof payload.search_engine === "string") {
        output.engine = payload.search_engine;
    }

    return Array.isArray(payload.items)
        || typeof payload.search_time_ms === "number"
        || typeof payload.search_engine === "string";
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

const parseSearchResult = (result?: string, fallbackSearchTimeMs?: number): ParsedSearchResult => {
    if (!result) {
        return {};
    }
    try {
        const parsed = JSON.parse(result);
        if (!parsed || typeof parsed !== "object") {
            return {};
        }

        const output: ParsedSearchResult = {
            searchTimeMs: fallbackSearchTimeMs,
        };
        if (isRecord(parsed)) {
            applyStructuredSearchPayload(parsed, output);
        }

        const contentParts = getToolContentParts(parsed);
        if (contentParts.length > 0) {
            for (const part of contentParts) {
                if (part.type === "json" && "json" in part) {
                    applyStructuredSearchPayload(part.json, output);
                }
                if (
                    part.type === "text" &&
                    typeof part.text === "string" &&
                    typeof output.resultCount !== "number"
                ) {
                    try {
                        applyStructuredSearchPayload(JSON.parse(part.text), output);
                    } catch {
                        // Markdown / HTML mode: estimate count from link patterns.
                        const estimated = estimateResultCountFromMarkdown(part.text);
                        if (typeof estimated === "number") {
                            output.resultCount = estimated;
                        }
                    }
                }
            }
        } else {
            applyStructuredSearchPayload(parsed, output);
            if (typeof output.resultCount !== "number" && isRecord(parsed) && typeof parsed.text === "string") {
                try {
                    applyStructuredSearchPayload(JSON.parse(parsed.text), output);
                } catch {
                    const estimated = estimateResultCountFromMarkdown(parsed.text);
                    if (typeof estimated === "number") {
                        output.resultCount = estimated;
                    }
                }
            }
        }

        return output;
    } catch {
        return {};
    }
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
            return "搜索中";
        case "success":
            return "搜索完成";
        case "failed":
            return "搜索失败";
        default:
            return "准备搜索";
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
        <span className="font-medium text-foreground truncate max-w-[180px]">{value}</span>
    </span>
);

const SearchWebToolCall: React.FC<McpToolComponentProps> = (props) => {
    const parsedParameters = useMemo(() => parseParameters(props.parameters), [props.parameters]);
    const query = stringValue(parsedParameters.query) || "未指定关键词";

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
    const isStreamingPlaceholder = props.isStreaming && !stateOverride?.status;
    const state = stateOverride?.status ?? (props.isStreaming ? "streaming" : localState);
    const effectiveResult = stateOverride?.result ?? (props.currentToolCall?.result);
    const fallbackSearchTimeMs = useMemo(
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
        () => parseSearchResult(effectiveResult, fallbackSearchTimeMs),
        [effectiveResult, fallbackSearchTimeMs],
    );

    const isRunning = Boolean(effectiveCallId && props.shiningMcpCallId === effectiveCallId);
    const shouldShine = isRunning || isStreamingPlaceholder || state === "executing";
    const isExecuting = state === "executing";
    const isFailed = state === "failed";
    const canExecute = state === "idle" || state === "pending" || state === "failed";
    const continueOnToolErrorEnabled = useToolErrorContinueEnabled();
    const isProtocolFailureWithoutCall = !effectiveCallId
        && isFailed
        && (props.status === "failed" || Boolean(props.error));
    const shouldHideFailedActions = isFailed && continueOnToolErrorEnabled;
    const canShowExecute = canExecute
        && !isStreamingPlaceholder
        && !shouldHideFailedActions
        && !isProtocolFailureWithoutCall;
    const canShowContinueWithError = isFailed
        && Boolean(effectiveCallId)
        && !isStreamingPlaceholder
        && !shouldHideFailedActions;
    const effectiveError = stateOverride?.error ?? localError ?? props.error ?? null;

    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    const shouldMask = antiLeakageEnabled && !isRevealed;
    const masked = shouldMask
        ? maskToolCall(props.serverName ?? "", props.toolName ?? "", props.parameters ?? "{}")
        : null;
    const displayQuery = shouldMask ? masked?.parameters ?? "******" : query;
    const displayError = shouldMask && effectiveError ? "******" : effectiveError;

    useEffect(() => {
        if (stateOverride?.status) {
            setLocalState(stateOverride.status);
            setLocalError(stateOverride.error ?? null);
            return;
        }
        if (props.isStreaming) {
            setLocalState("streaming");
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
            console.warn("[SearchWebToolCall] focus sidebar failed", error);
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
        <MotionToolCard
            interactive={canFocusInSidebar}
            onClick={handleCardClick}
            onKeyDown={handleCardKeyDown}
            role={canFocusInSidebar ? "button" : undefined}
            tabIndex={canFocusInSidebar ? 0 : undefined}
            aria-label={canFocusInSidebar ? "在侧栏中定位搜索详情" : undefined}
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
                        <Search className="mt-0.5 h-4 w-4 flex-shrink-0 text-muted-foreground" />
                        <div className="min-w-0 flex-1">
                            <div className="truncate text-sm font-medium text-foreground" title={displayQuery}>
                                {displayQuery}
                            </div>
                            <div className="text-xs text-muted-foreground mt-0.5">网络搜索</div>
                        </div>
                    </div>
                    <div className="flex flex-shrink-0 items-center gap-1">
                        <div title={displayError ?? undefined}>
                            <MotionStatusSlot stateKey={state}>
                                <StatusBadge state={state} />
                            </MotionStatusSlot>
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

                <MotionMetaRow
                    show={state === "success" && (Boolean(parsedResult.engine) || typeof parsedResult.resultCount === "number" || typeof parsedResult.searchTimeMs === "number")}
                    className="flex flex-wrap items-center gap-x-3 gap-y-1"
                >
                        {parsedResult.engine && (
                            <MotionMetricItem metricKey={`engine-${parsedResult.engine}`}>
                                <MetaItem
                                    icon={<Globe className="h-3 w-3" />}
                                    value={parsedResult.engine}
                                    title={`搜索引擎：${parsedResult.engine}`}
                                />
                            </MotionMetricItem>
                        )}
                        {typeof parsedResult.resultCount === "number" && (
                            <MotionMetricItem metricKey={`count-${parsedResult.resultCount}`}>
                                <MetaItem
                                    icon={<Hash className="h-3 w-3" />}
                                    value={`${parsedResult.resultCount} 条`}
                                    title={`结果数量：${parsedResult.resultCount} 条`}
                                />
                            </MotionMetricItem>
                        )}
                        {typeof parsedResult.searchTimeMs === "number" && (
                            <MotionMetricItem metricKey={`time-${parsedResult.searchTimeMs}`}>
                                <MetaItem
                                    icon={<Clock className="h-3 w-3" />}
                                    value={formatDurationSeconds(parsedResult.searchTimeMs)}
                                    title={`搜索耗时：${formatDurationSeconds(parsedResult.searchTimeMs)}`}
                                />
                            </MotionMetricItem>
                        )}
                </MotionMetaRow>

                <MotionDetails show={state === "failed" && Boolean(displayError)}>
                    <div
                        className="max-h-24 overflow-auto whitespace-pre-wrap break-words rounded-md border border-destructive/20 bg-destructive/5 px-2 py-1.5 text-xs text-destructive"
                        title={displayError ?? undefined}
                    >
                        {displayError}
                    </div>
                </MotionDetails>
            </div>
        </MotionToolCard>
    );
};

export default SearchWebToolCall;
