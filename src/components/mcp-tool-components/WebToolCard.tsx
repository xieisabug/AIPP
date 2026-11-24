import React, { useCallback, useEffect, useMemo, useState } from "react";
import {
    ArrowRight,
    CheckCircle,
    FileSearch,
    Globe,
    Loader2,
    Play,
    RotateCcw,
    Search,
    Square,
    XCircle,
    type LucideIcon,
} from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ShineBorder } from "@/components/magicui/shine-border";
import { MotionDetails, MotionMetricItem, MotionStatusSlot, MotionToolCard } from "@/components/mcp-tool-components/McpToolMotion";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import { useToolErrorContinueEnabled } from "@/components/McpToolCall";
import { useAntiLeakage } from "@/contexts/AntiLeakageContext";
import { maskToolCall } from "@/utils/antiLeakage";
import { getErrorMessage } from "@/utils/error";
import type { MCPToolCall } from "@/data/MCPToolCall";
import type { McpToolCallStatus, McpToolComponentProps } from "@/services/mcpToolComponentRegistry";

/**
 * 联网类工具（AIPP 内置 search_web / fetch_url、DeepSeek 服务端搜索与阅读链接）共用的卡片。
 * 不管工具在哪里执行，用户看到的都是同一种卡：图标 + 查询/网址 + 一行摘要 + 状态，点击在侧栏查看来源。
 */

export type WebToolState = McpToolCallStatus | "streaming" | "idle";
export type WebToolKind = "search" | "fetch" | "read" | "find";

const KINDS: Record<WebToolKind, { icon: LucideIcon; label: string; verb: string }> = {
    search: { icon: Search, label: "网络搜索", verb: "搜索" },
    fetch: { icon: Globe, label: "抓取网页", verb: "抓取" },
    read: { icon: Globe, label: "阅读网页", verb: "阅读" },
    find: { icon: FileSearch, label: "页内查找", verb: "查找" },
};

export interface WebToolMeta {
    key: string;
    value: string;
    title: string;
}

const statusLabel = (state: WebToolState, verb: string): string => {
    switch (state) {
        case "streaming":
            return "生成中";
        case "pending":
            return "待执行";
        case "executing":
            return `${verb}中`;
        case "success":
            return `${verb}完成`;
        case "failed":
            return `${verb}失败`;
        default:
            return `准备${verb}`;
    }
};

const StatusBadge: React.FC<{ state: WebToolState; verb: string }> = ({ state, verb }) => {
    const label = statusLabel(state, verb);
    if (state === "success") {
        return (
            <Badge variant="default" className="flex items-center gap-1 bg-success text-success-foreground border-success-border">
                <CheckCircle className="h-3 w-3 text-success-foreground" />
                {label}
            </Badge>
        );
    }
    if (state === "failed") {
        return (
            <Badge variant="destructive" className="flex items-center gap-1">
                <XCircle className="h-3 w-3" />
                {label}
            </Badge>
        );
    }
    if (state === "executing" || state === "streaming") {
        return (
            <Badge variant="secondary" className="flex items-center gap-1">
                <Loader2 className="h-3 w-3 animate-spin" />
                {label}
            </Badge>
        );
    }
    return <Badge variant="outline">{label}</Badge>;
};

export interface WebToolCardProps {
    kind: WebToolKind;
    title: string;
    /** 悬停显示的完整内容（多条查询、完整网址），默认同 title。 */
    titleTooltip?: string;
    state: WebToolState;
    /** 摘要行在类型名之后依次显示的指标（引擎、结果数、耗时、站点）。 */
    meta?: WebToolMeta[];
    error?: string | null;
    shine?: boolean;
    animateEntrance?: boolean;
    actions?: React.ReactNode;
    /** 提供时整张卡可点击（在侧栏中定位来源）。 */
    onOpen?: () => void;
    openLabel?: string;
}

export const WebToolCard: React.FC<WebToolCardProps> = ({
    kind,
    title,
    titleTooltip,
    state,
    meta = [],
    error,
    shine = false,
    animateEntrance = false,
    actions,
    onOpen,
    openLabel,
}) => {
    const { icon: Icon, label, verb } = KINDS[kind];
    const handleKeyDown = useCallback((event: React.KeyboardEvent<HTMLDivElement>) => {
        if (!onOpen) return;
        if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            onOpen();
        }
    }, [onOpen]);

    return (
        <MotionToolCard
            animateEntrance={animateEntrance}
            interactive={Boolean(onOpen)}
            onClick={onOpen}
            onKeyDown={onOpen ? handleKeyDown : undefined}
            role={onOpen ? "button" : undefined}
            tabIndex={onOpen ? 0 : undefined}
            aria-label={onOpen ? openLabel : undefined}
            data-aipp-slot="web-tool-card"
            data-web-tool-kind={kind}
        >
            {shine && (
                <ShineBorder
                    shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                    borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                    duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                />
            )}
            <div className="flex items-center gap-2.5">
                <div
                    className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md bg-muted text-muted-foreground"
                    data-aipp-slot="web-tool-card-icon"
                    aria-hidden="true"
                >
                    <Icon className="h-4 w-4" />
                </div>
                <div className="min-w-0 flex-1">
                    <div className="truncate text-sm font-medium leading-5 text-foreground" title={titleTooltip ?? title}>
                        {title}
                    </div>
                    <div className="flex min-w-0 items-center gap-1.5 overflow-hidden text-xs leading-4 text-muted-foreground">
                        <span className="flex-shrink-0">{label}</span>
                        {meta.map((item) => (
                            <MotionMetricItem key={item.key} metricKey={`${item.key}-${item.value}`}>
                                <span
                                    className="min-w-0 truncate before:mr-1.5 before:content-['·']"
                                    title={item.title}
                                >
                                    {item.value}
                                </span>
                            </MotionMetricItem>
                        ))}
                    </div>
                </div>
                <div className="flex flex-shrink-0 items-center gap-1">
                    <div title={error ?? undefined}>
                        <MotionStatusSlot stateKey={state}>
                            <StatusBadge state={state} verb={verb} />
                        </MotionStatusSlot>
                    </div>
                    {actions}
                </div>
            </div>
            <MotionDetails show={state === "failed" && Boolean(error)}>
                <div
                    className="mt-2 max-h-24 overflow-auto whitespace-pre-wrap break-words rounded-md border border-destructive/20 bg-destructive/5 px-2 py-1.5 text-xs text-destructive"
                    title={error ?? undefined}
                >
                    {error}
                </div>
            </MotionDetails>
        </MotionToolCard>
    );
};

// ---- helpers shared by the MCP-backed cards ----

export const parseToolParameters = (parameters?: string): Record<string, unknown> => {
    if (!parameters) return {};
    try {
        const parsed = JSON.parse(parameters);
        return parsed && typeof parsed === "object" && !Array.isArray(parsed)
            ? (parsed as Record<string, unknown>)
            : {};
    } catch {
        return {};
    }
};

const timestampMs = (value?: Date | string): number | undefined => {
    if (!value) return undefined;
    const time = value instanceof Date ? value.getTime() : new Date(value).getTime();
    return Number.isFinite(time) ? time : undefined;
};

export const formatDurationSeconds = (durationMs: number): string => {
    const seconds = durationMs / 1000;
    if (seconds >= 10) {
        return `${Math.round(seconds)} 秒`;
    }
    return `${Number(seconds.toFixed(1))} 秒`;
};

/**
 * search_web / fetch_url 卡片共用的执行状态：匹配持久化状态、执行/停止/以错误继续、防泄露遮盖与侧栏定位。
 */
export function useWebToolCall(props: McpToolComponentProps, logTag: string) {
    const [localState, setLocalState] = useState<WebToolState>(
        props.isStreaming ? "streaming" : props.currentToolCall?.status ?? props.status ?? "idle",
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
    const isStreamingPlaceholder = Boolean(props.isStreaming && !stateOverride?.status);
    const state: WebToolState = stateOverride?.status ?? (props.isStreaming ? "streaming" : localState);
    const result = stateOverride?.result ?? props.currentToolCall?.result;
    const startedTime = stateOverride?.started_time ?? props.currentToolCall?.started_time;
    const finishedTime = stateOverride?.finished_time ?? props.currentToolCall?.finished_time;
    const elapsedMs = useMemo(() => {
        const started = timestampMs(startedTime);
        const finished = timestampMs(finishedTime);
        if (typeof started !== "number" || typeof finished !== "number" || finished < started) {
            return undefined;
        }
        return finished - started;
    }, [startedTime, finishedTime]);

    const isRunning = Boolean(effectiveCallId && props.shiningMcpCallId === effectiveCallId);
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
    const masked = antiLeakageEnabled && !isRevealed;
    const maskedParameters = masked
        ? maskToolCall(props.serverName ?? "", props.toolName ?? "", props.parameters ?? "{}").parameters || "******"
        : null;

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
            const executed = await invoke<MCPToolCall>("execute_mcp_tool_call", {
                callId: currentCallId,
                triggerContinuation: true,
            });
            setLocalState(executed.status);
            setLocalError(executed.error ?? null);
        } catch (error) {
            setLocalState("failed");
            setLocalError(getErrorMessage(error) || "执行失败");
        }
    }, [effectiveCallId, props.conversationId, props.messageId, props.serverName, props.toolName, props.parameters]);

    const handleStop = useCallback(async (event?: React.MouseEvent) => {
        event?.stopPropagation();
        if (!effectiveCallId) return;
        try {
            await invoke("stop_mcp_tool_call", { callId: effectiveCallId });
        } catch (error) {
            setLocalState("failed");
            setLocalError(getErrorMessage(error) || "停止失败");
        }
    }, [effectiveCallId]);

    const handleContinueWithError = useCallback(async (event?: React.MouseEvent) => {
        event?.stopPropagation();
        if (!effectiveCallId) return;
        try {
            await invoke("continue_with_error", { callId: effectiveCallId, errorMessage: effectiveError });
        } catch (error) {
            setLocalError(getErrorMessage(error) || "继续失败");
        }
    }, [effectiveCallId, effectiveError]);

    const canOpen = state === "success" && effectiveCallId !== null;
    const handleOpen = useCallback(() => {
        if (!effectiveCallId) return;
        emit("sidebar-focus-context", { id: `mcp-${effectiveCallId}` }).catch((error) => {
            console.warn(`[${logTag}] focus sidebar failed`, error);
        });
    }, [effectiveCallId, logTag]);

    const actions = (
        <>
            {isExecuting && effectiveCallId && (
                <Button onClick={handleStop} size="sm" variant="ghost" className="h-7 w-7 flex-shrink-0 p-0 text-destructive" iconLabel="停止">
                    <Square className="h-3 w-3 fill-current" />
                </Button>
            )}
            {canShowExecute && (
                <Button onClick={handleExecute} size="sm" variant="ghost" className="h-7 w-7 flex-shrink-0 p-0" iconLabel={isFailed ? "重新执行" : "执行"}>
                    {isFailed ? <RotateCcw className="h-3 w-3" /> : <Play className="h-3 w-3" />}
                </Button>
            )}
            {canShowContinueWithError && (
                <Button onClick={handleContinueWithError} size="sm" variant="ghost" className="h-7 w-7 flex-shrink-0 p-0" iconLabel="以错误继续对话">
                    <ArrowRight className="h-3 w-3" />
                </Button>
            )}
        </>
    );

    return {
        state,
        result,
        elapsedMs,
        animateEntrance: isRunning,
        shine: isRunning || isStreamingPlaceholder || isExecuting,
        error: masked && effectiveError ? "******" : effectiveError,
        maskedParameters,
        actions,
        onOpen: canOpen ? handleOpen : undefined,
    };
}
