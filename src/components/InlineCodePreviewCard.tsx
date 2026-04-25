import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { MCPToolCall } from "@/data/MCPToolCall";
import { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { getErrorMessage } from "@/utils/error";
import {
    buildPreviewCodeSignature,
    parsePreviewCodeRequestLoose,
    type PreviewCodeStreamingState,
    parsePreviewCodeToolResult,
    PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX,
    PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS,
    PreviewCodeRequestEvent,
} from "@/utils/previewCode";
import { createPreviewCodeRuntime } from "@/utils/previewCodeRuntime";
import { CheckCircle, Loader2, Sparkles, XCircle } from "lucide-react";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";

interface InlineCodePreviewCardProps {
    parameters: string;
    llmCallId?: string;
    conversationId?: number;
    messageId?: number;
    callId?: number;
    isLastMessage?: boolean;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    isStreaming?: boolean;
    streamingPreviewState?: PreviewCodeStreamingState;
}

type DisplayState =
    | "streaming"
    | "pending"
    | "executing"
    | "submitted"
    | "dismissed"
    | "failed"
    | "idle";

export default function InlineCodePreviewCard({
    parameters,
    llmCallId,
    conversationId,
    messageId,
    callId,
    isLastMessage = false,
    mcpToolCallStates,
    isStreaming = false,
    streamingPreviewState,
}: InlineCodePreviewCardProps) {
    const hostRef = useRef<HTMLDivElement | null>(null);
    const runtimeRef = useRef<ReturnType<typeof createPreviewCodeRuntime> | null>(null);
    const { isPreviewCodeShowToolbar } = useDisplayConfig();
    const [resolvedRequestId, setResolvedRequestId] = useState<string | null>(null);
    const [persistedToolCall, setPersistedToolCall] = useState<MCPToolCall | null>(null);
    const [runtimeError, setRuntimeError] = useState<string | null>(null);
    const [interactionError, setInteractionError] = useState<string | null>(null);
    const [isSubmitting, setIsSubmitting] = useState(false);
    const [isCollapsed, setIsCollapsed] = useState(!isLastMessage && !isStreaming);
    const [isHidden, setIsHidden] = useState(false);
    const [isRuntimeActivated, setIsRuntimeActivated] = useState(
        isStreaming || isLastMessage,
    );
    const [optimisticResult, setOptimisticResult] = useState<ReturnType<typeof parsePreviewCodeToolResult> | null>(null);
    const [displayStreamingPreviewState, setDisplayStreamingPreviewState] =
        useState<PreviewCodeStreamingState | null>(streamingPreviewState ?? null);
    const displayStreamingPreviewStateRef = useRef<PreviewCodeStreamingState | null>(
        streamingPreviewState ?? null,
    );
    const pendingStreamingPreviewStateRef = useRef<PreviewCodeStreamingState | null>(null);
    const streamingPreviewTimerRef = useRef<number | null>(null);
    const lastStreamingPreviewAppliedAtRef = useRef(0);
    const lastStreamingPreviewIdentityRef = useRef<string | null>(null);

    const matchedStateByLlmCallId = useMemo(() => {
        if (!mcpToolCallStates || !llmCallId || callId) {
            return undefined;
        }
        for (const state of mcpToolCallStates.values()) {
            if (state.llm_call_id === llmCallId) {
                return state;
            }
        }
        return undefined;
    }, [mcpToolCallStates, llmCallId, callId]);

    const effectiveCallId = callId ?? matchedStateByLlmCallId?.call_id ?? null;
    const previewIdentity = useMemo(
        () =>
            effectiveCallId !== null
                ? `call:${effectiveCallId}`
                : llmCallId
                  ? `llm:${llmCallId}`
                  : messageId !== undefined
                    ? `message:${messageId}`
                    : "transient",
        [effectiveCallId, llmCallId, messageId]
    );
    const activeStreamingPreviewState =
        isStreaming ? (displayStreamingPreviewState ?? streamingPreviewState ?? null) : streamingPreviewState;
    const stateOverride =
        (effectiveCallId && mcpToolCallStates
            ? mcpToolCallStates.get(effectiveCallId)
            : undefined) ?? matchedStateByLlmCallId;
    const effectiveParameters = isStreaming
        ? parameters
        : stateOverride?.parameters ?? persistedToolCall?.parameters ?? parameters;

    useEffect(() => {
        const clearScheduledStreamingPreview = () => {
            if (streamingPreviewTimerRef.current !== null) {
                window.clearTimeout(streamingPreviewTimerRef.current);
                streamingPreviewTimerRef.current = null;
            }
        };
        const applyStreamingPreviewState = (nextState: PreviewCodeStreamingState | null) => {
            clearScheduledStreamingPreview();
            pendingStreamingPreviewStateRef.current = null;
            displayStreamingPreviewStateRef.current = nextState;
            setDisplayStreamingPreviewState(nextState);
            lastStreamingPreviewAppliedAtRef.current = nextState ? Date.now() : 0;
        };

        if (!isStreaming) {
            lastStreamingPreviewIdentityRef.current = previewIdentity;
            applyStreamingPreviewState(streamingPreviewState ?? null);
            return;
        }

        if (!streamingPreviewState) {
            lastStreamingPreviewIdentityRef.current = previewIdentity;
            applyStreamingPreviewState(null);
            return;
        }

        if (lastStreamingPreviewIdentityRef.current !== previewIdentity) {
            lastStreamingPreviewIdentityRef.current = previewIdentity;
            applyStreamingPreviewState(streamingPreviewState);
            return;
        }

        // Read the current visible state from the ref to avoid a feedback loop:
        // displayStreamingPreviewState (React state) is intentionally excluded from
        // the deps to prevent the effect from re-triggering on every apply cycle.
        const currentVisibleState = displayStreamingPreviewStateRef.current;
        if (!currentVisibleState) {
            applyStreamingPreviewState(streamingPreviewState);
            return;
        }

        const isUnchanged =
            currentVisibleState.title === streamingPreviewState.title
            && currentVisibleState.renderer === streamingPreviewState.renderer
            && currentVisibleState.interactionMode === streamingPreviewState.interactionMode
            && currentVisibleState.hasRenderableDom === streamingPreviewState.hasRenderableDom
            && currentVisibleState.containsScript === streamingPreviewState.containsScript
            && currentVisibleState.renderableHtml === streamingPreviewState.renderableHtml
            && currentVisibleState.sourceExcerpt === streamingPreviewState.sourceExcerpt
            && currentVisibleState.loadingMessages.join("\u0000")
                === streamingPreviewState.loadingMessages.join("\u0000");
        if (isUnchanged) {
            return;
        }

        if (!currentVisibleState.hasRenderableDom && streamingPreviewState.hasRenderableDom) {
            applyStreamingPreviewState(streamingPreviewState);
            return;
        }

        const elapsed = Date.now() - lastStreamingPreviewAppliedAtRef.current;
        if (elapsed >= PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS) {
            applyStreamingPreviewState(streamingPreviewState);
            return;
        }

        pendingStreamingPreviewStateRef.current = streamingPreviewState;
        if (streamingPreviewTimerRef.current !== null) {
            return;
        }
        streamingPreviewTimerRef.current = window.setTimeout(() => {
            streamingPreviewTimerRef.current = null;
            const pendingState = pendingStreamingPreviewStateRef.current;
            pendingStreamingPreviewStateRef.current = null;
            displayStreamingPreviewStateRef.current = pendingState;
            setDisplayStreamingPreviewState(pendingState);
            lastStreamingPreviewAppliedAtRef.current = pendingState ? Date.now() : 0;
        }, Math.max(PREVIEW_CODE_STREAMING_UPDATE_INTERVAL_MS - elapsed, 0));
    }, [isStreaming, previewIdentity, streamingPreviewState]);

    useEffect(() => {
        return () => {
            if (streamingPreviewTimerRef.current !== null) {
                window.clearTimeout(streamingPreviewTimerRef.current);
            }
        };
    }, []);

    const previewRequest = useMemo(() => {
        if (isStreaming && activeStreamingPreviewState) {
            return activeStreamingPreviewState;
        }
        return parsePreviewCodeRequestLoose(effectiveParameters);
    }, [activeStreamingPreviewState, effectiveParameters, isStreaming]);
    const requestSignature = useMemo(
        () =>
            isStreaming
                ? null
                : buildPreviewCodeSignature(
                      previewRequest
                          ? {
                                title: previewRequest.title,
                                renderer: previewRequest.renderer,
                                code: previewRequest.code,
                                interactionMode: previewRequest.interactionMode,
                            }
                          : null
                  ),
        [isStreaming, previewRequest]
    );
    const hasScriptContent = useMemo(() => {
        if (activeStreamingPreviewState) {
            return activeStreamingPreviewState.containsScript;
        }
        return /<script[\s>]/i.test(previewRequest?.code ?? "");
    }, [activeStreamingPreviewState, previewRequest?.code]);
    const isInteractionEnabled = isStreaming || !hasScriptContent || !isCollapsed;
    const toolResult =
        optimisticResult ??
        parsePreviewCodeToolResult(stateOverride?.result ?? persistedToolCall?.result ?? null);

    const displayState: DisplayState = useMemo(() => {
        if (toolResult?.status === "submitted") {
            return "submitted";
        }
        if (toolResult?.status === "dismissed") {
            return "dismissed";
        }
        const status = stateOverride?.status ?? persistedToolCall?.status;
        if (status === "failed") {
            return "failed";
        }
        if (status === "executing") {
            return "executing";
        }
        if (status === "pending") {
            return "pending";
        }
        if (isStreaming) {
            return "streaming";
        }
        return "idle";
    }, [toolResult, stateOverride?.status, persistedToolCall?.status, isStreaming]);
    const loadingMessage = useMemo(() => {
        if (!previewRequest?.loadingMessages?.length) {
            return null;
        }
        if (!previewRequest.code.trim()) {
            return previewRequest.loadingMessages[0];
        }
        if (previewRequest.code.includes("<script")) {
            return (
                previewRequest.loadingMessages[
                    Math.min(previewRequest.loadingMessages.length - 1, 2)
                ] ?? previewRequest.loadingMessages[previewRequest.loadingMessages.length - 1]
            );
        }
        return (
            previewRequest.loadingMessages[
                Math.min(previewRequest.loadingMessages.length - 1, 1)
            ] ?? previewRequest.loadingMessages[0]
        );
    }, [previewRequest]);
    const shouldShowStreamingFallback =
        isStreaming && !!activeStreamingPreviewState && !activeStreamingPreviewState.hasRenderableDom;
    const sourceExcerpt = useMemo(() => {
        if (activeStreamingPreviewState?.sourceExcerpt?.trim()) {
            return activeStreamingPreviewState.sourceExcerpt.trim();
        }
        const code = previewRequest?.code?.trim();
        if (!code) {
            return null;
        }
        return code.length > 600 ? `${code.slice(0, 600)}…` : code;
    }, [activeStreamingPreviewState?.sourceExcerpt, previewRequest?.code]);
    const previewHidden = displayState === "dismissed" || isHidden;
    const hostHidden = previewHidden || shouldShowStreamingFallback;
    const shouldRenderPreviewSurface = !previewHidden && !shouldShowStreamingFallback;
    const shouldRenderRuntimeHost = !previewHidden && !shouldShowStreamingFallback;
    const previewViewportHeight = isStreaming
        ? Math.max(160, PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX - 64)
        : PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX;

    useEffect(() => {
        const defaultCollapsed = !isLastMessage && !isStreaming;
        setResolvedRequestId(null);
        setOptimisticResult(null);
        setInteractionError(null);
        setRuntimeError(null);
        setIsCollapsed(defaultCollapsed);
        setIsHidden(false);
        setIsRuntimeActivated(isStreaming || !defaultCollapsed);
    }, [conversationId, hasScriptContent, isLastMessage, isStreaming, requestSignature]);

    useEffect(() => {
        if (!effectiveCallId) {
            setPersistedToolCall(null);
            return;
        }
        let active = true;
        invoke<MCPToolCall>("get_mcp_tool_call", { callId: effectiveCallId })
            .then((toolCall) => {
                if (active) {
                    setPersistedToolCall(toolCall);
                }
            })
            .catch((error) => {
                if (active) {
                    console.warn("Failed to load preview_code tool call:", getErrorMessage(error));
                }
            });
        return () => {
            active = false;
        };
    }, [effectiveCallId]);

    useEffect(() => {
        if (!conversationId || !requestSignature) {
            return;
        }
        let active = true;
        invoke<PreviewCodeRequestEvent[]>("list_preview_code_requests_for_conversation", {
            conversationId,
            conversation_id: conversationId,
        })
            .then((events) => {
                if (!active) {
                    return;
                }
                const matched = events.find((event) => {
                    const signature = buildPreviewCodeSignature({
                        title: event.title,
                        renderer: event.renderer,
                        code: event.code,
                        interactionMode: event.interactionMode,
                    });
                    return signature === requestSignature;
                });
                if (matched) {
                    setResolvedRequestId(matched.request_id);
                }
            })
            .catch((error) => {
                console.warn(
                    "Failed to load pending preview_code requests:",
                    getErrorMessage(error)
                );
            });
        return () => {
            active = false;
        };
    }, [conversationId, requestSignature]);

    useEffect(() => {
        if (!conversationId || !requestSignature) {
            return;
        }
        const unsubscribe = listen<PreviewCodeRequestEvent>("preview-code-request", (event) => {
            const payload = event.payload;
            if (
                conversationId !== undefined &&
                payload.conversation_id !== undefined &&
                payload.conversation_id !== null &&
                payload.conversation_id !== conversationId
            ) {
                return;
            }
            const signature = buildPreviewCodeSignature({
                title: payload.title,
                renderer: payload.renderer,
                code: payload.code,
                interactionMode: payload.interactionMode,
            });
            if (signature === requestSignature) {
                setResolvedRequestId(payload.request_id);
            }
        });

        return () => {
            unsubscribe.then((dispose) => dispose());
        };
    }, [conversationId, requestSignature]);

    useEffect(() => {
        if (!hostRef.current || runtimeRef.current || !isRuntimeActivated || !shouldRenderRuntimeHost) {
            return;
        }
        runtimeRef.current = createPreviewCodeRuntime(hostRef.current);
        return () => {
            runtimeRef.current?.destroy();
            runtimeRef.current = null;
        };
    }, [isRuntimeActivated, shouldRenderRuntimeHost]);

    useEffect(() => {
        if (
            !previewRequest
            || !runtimeRef.current
            || !isRuntimeActivated
            || !shouldRenderRuntimeHost
        ) {
            return;
        }

        const bridgeId =
            `preview-code-${llmCallId ?? messageId ?? effectiveCallId ?? "transient"}`;
        const renderCode =
            isStreaming && activeStreamingPreviewState
                ? activeStreamingPreviewState.renderableHtml
                : previewRequest.code;
        runtimeRef.current.update({
            code: renderCode,
            isFinal: !isStreaming && (!hasScriptContent || isInteractionEnabled),
            bridgeId,
            bridge: {
                submit: async (payload?: unknown) => {
                    if (previewRequest.interactionMode === "none") {
                        setInteractionError("当前 preview_code 不允许提交结果");
                        return;
                    }
                    if (!resolvedRequestId) {
                        setInteractionError("preview_code 尚未绑定 request_id，请稍后重试");
                        return;
                    }
                    setIsSubmitting(true);
                    setInteractionError(null);
                    try {
                        await invoke("submit_preview_code_response", {
                            requestId: resolvedRequestId,
                            request_id: resolvedRequestId,
                            payload,
                            dismissed: false,
                        });
                        setOptimisticResult({
                            status: "submitted",
                            request_id: resolvedRequestId,
                            payload,
                        });
                    } catch (error) {
                        setInteractionError(getErrorMessage(error));
                    } finally {
                        setIsSubmitting(false);
                    }
                },
                close: async () => {
                    if (!resolvedRequestId) {
                        setInteractionError("preview_code 尚未绑定 request_id，请稍后重试");
                        return;
                    }
                    setIsSubmitting(true);
                    setInteractionError(null);
                    try {
                        await invoke("submit_preview_code_response", {
                            requestId: resolvedRequestId,
                            request_id: resolvedRequestId,
                            payload: null,
                            dismissed: true,
                        });
                        setOptimisticResult({
                            status: "dismissed",
                            request_id: resolvedRequestId,
                        });
                    } catch (error) {
                        setInteractionError(getErrorMessage(error));
                    } finally {
                        setIsSubmitting(false);
                    }
                },
                emitEvent: (_name: string, _payload?: unknown) => undefined,
            },
            onError: setRuntimeError,
        });
    }, [
        previewRequest,
        effectiveCallId,
        llmCallId,
        messageId,
        isStreaming,
        resolvedRequestId,
        activeStreamingPreviewState,
            isRuntimeActivated,
            hasScriptContent,
            isInteractionEnabled,
            isLastMessage,
            shouldRenderRuntimeHost,
        ]);

    useEffect(() => {
        if (displayState === "streaming" || displayState === "pending" || displayState === "executing") {
            setIsCollapsed(false);
            setIsHidden(false);
            setIsRuntimeActivated(true);
        }
    }, [displayState]);

    const statusBadge = (() => {
        switch (displayState) {
            case "streaming":
                return (
                    <Badge variant="secondary" className="flex items-center gap-1">
                        <Loader2 className="h-3 w-3 animate-spin" />
                        生成中
                    </Badge>
                );
            case "pending":
            case "executing":
                return null;
            case "submitted":
                return (
                    <Badge className="flex items-center gap-1 bg-success text-success-foreground border-success-border">
                        <CheckCircle className="h-3 w-3" />
                        已提交
                    </Badge>
                );
            case "dismissed":
                return (
                    <Badge variant="outline" className="flex items-center gap-1">
                        <XCircle className="h-3 w-3" />
                        已关闭
                    </Badge>
                );
            case "failed":
                return (
                    <Badge variant="destructive" className="flex items-center gap-1">
                        <XCircle className="h-3 w-3" />
                        失败
                    </Badge>
                );
            default:
                return (
                    <Badge variant="outline" className="flex items-center gap-1">
                        <Sparkles className="h-3 w-3" />
                        就绪
                    </Badge>
                );
        }
    })();

    useEffect(() => {
        if (hostHidden || isRuntimeActivated) {
            return;
        }

        const host = hostRef.current;
        if (!host) {
            return;
        }

        const root = host.closest(
            "[data-aipp-slot='chat-conversation-scroll']",
        ) as Element | null;
        if (!root || typeof IntersectionObserver !== "function") {
            setIsRuntimeActivated(true);
            return;
        }
        const observer = new IntersectionObserver(
            (entries) => {
                if (entries.some((entry) => entry.isIntersecting)) {
                    setIsRuntimeActivated(true);
                    observer.disconnect();
                }
            },
            {
                root,
                rootMargin: "320px 0px",
                threshold: 0.01,
            },
        );
        observer.observe(host);

        return () => {
            observer.disconnect();
        };
    }, [hostHidden, isRuntimeActivated]);

    useEffect(() => {
        if (shouldRenderRuntimeHost) {
            return;
        }
        runtimeRef.current?.destroy();
        runtimeRef.current = null;
    }, [shouldRenderRuntimeHost]);

    useEffect(() => {
        if (!previewHidden) {
            return;
        }
        setRuntimeError(null);
    }, [previewHidden]);

    return (
        <div className={isPreviewCodeShowToolbar ? "space-y-3 py-2" : "py-1"}>
            {isPreviewCodeShowToolbar && (
                <div className="space-y-3">
                    <div className="flex items-start justify-between gap-3">
                        <div className="space-y-1">
                            <div className="text-sm font-medium">
                                {previewRequest?.title || "inline_preview"}
                            </div>
                            <div className="text-xs text-muted-foreground">
                                preview_code · {previewRequest?.renderer || "html"}
                            </div>
                        </div>
                        <div className="flex items-center gap-2">
                            {statusBadge}
                            {displayState !== "dismissed" && (
                                <Button
                                    type="button"
                                    variant="ghost"
                                    size="sm"
                                    onClick={() => setIsHidden((current) => !current)}
                                    disabled={isSubmitting}
                                >
                                    {isHidden ? "显示" : "隐藏"}
                                </Button>
                            )}
                        </div>
                    </div>
                    {loadingMessage && (displayState === "streaming" || displayState === "executing" || displayState === "pending") && (
                        <div className="text-xs text-muted-foreground">{loadingMessage}</div>
                    )}
                </div>
            )}
            <div className="space-y-3">
                {runtimeError && (
                    <div className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                        {runtimeError}
                    </div>
                )}
                {interactionError && (
                    <div className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                        {interactionError}
                    </div>
                )}
                {shouldRenderPreviewSurface && (
                    <div
                        className={isCollapsed
                            ? "relative overflow-hidden bg-transparent"
                            : "relative bg-transparent"}
                        style={{
                            overflowAnchor: "none",
                            ...(isCollapsed
                                ? {
                                    height: previewViewportHeight,
                                    minHeight: previewViewportHeight,
                                    maxHeight: previewViewportHeight,
                                }
                                : {}),
                        }}
                    >
                        <div
                            ref={hostRef}
                            data-testid="preview-code-host"
                            className={hostHidden
                                ? "hidden bg-transparent"
                                : isCollapsed
                                    ? "h-full overflow-hidden bg-transparent"
                                    : "bg-transparent"}
                            style={{
                                overflowAnchor: "none",
                                ...(hostHidden
                                    ? {}
                                    : isCollapsed
                                        ? {
                                            height: previewViewportHeight,
                                            minHeight: previewViewportHeight,
                                            maxHeight: previewViewportHeight,
                                        }
                                        : {}),
                            }}
                        >
                            {!hostHidden && !isRuntimeActivated && (
                                <div className="flex h-full items-center justify-center rounded-md border border-dashed border-border/60 bg-muted/20 px-4 text-xs text-muted-foreground">
                                    滚动到此处后加载预览
                                </div>
                            )}
                        </div>
                        {isCollapsed && (
                            <button
                                type="button"
                                className="absolute inset-0 z-10 flex flex-col items-center justify-end gap-2 bg-gradient-to-t from-background/95 via-background/72 to-background/18 px-4 pb-5 pt-10 text-center transition-colors hover:from-background hover:via-background/78 hover:to-background/26"
                                onClick={() => {
                                    setIsCollapsed(false);
                                    setIsRuntimeActivated(true);
                                }}
                                disabled={isSubmitting}
                                aria-label="展开预览"
                            >
                                <span className="text-sm font-medium text-foreground">点击展开预览</span>
                                <span className="text-xs text-muted-foreground">
                                    {hasScriptContent ? "展开后将启用交互" : "展开查看完整内容"}
                                </span>
                            </button>
                        )}
                        {!isCollapsed && (
                            <div className="absolute bottom-2 right-2 z-10">
                                <button
                                    type="button"
                                    className="pointer-events-auto rounded bg-muted px-2 py-1 text-xs text-foreground hover:bg-muted/80"
                                    onClick={() => setIsCollapsed(true)}
                                    disabled={isSubmitting}
                                    title="收起"
                                >
                                    收起
                                </button>
                            </div>
                        )}
                    </div>
                )}
                {shouldShowStreamingFallback && !previewHidden && sourceExcerpt && (
                    <div className="rounded-md border border-border/70 bg-muted/40 px-3 py-3 space-y-2">
                        <div className="text-xs text-muted-foreground">
                            正在生成可渲染预览，先展示当前代码片段。
                        </div>
                        <pre className="text-xs whitespace-pre-wrap break-words text-foreground/80">
                            {sourceExcerpt}
                        </pre>
                    </div>
                )}
                {toolResult?.status === "submitted" && toolResult.payload !== undefined && (
                    <pre className="rounded-md bg-muted p-3 text-xs whitespace-pre-wrap break-words">
                        {JSON.stringify(toolResult.payload, null, 2)}
                    </pre>
                )}
                {isHidden && displayState !== "dismissed" && (
                    <div className="text-sm text-muted-foreground">
                        预览已隐藏。
                    </div>
                )}
                {displayState === "dismissed" && (
                    <div className="text-sm text-muted-foreground">
                        该内嵌 UI 已关闭。
                    </div>
                )}
            </div>
        </div>
    );
}
