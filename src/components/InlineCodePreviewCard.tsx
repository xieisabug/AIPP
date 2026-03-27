import { useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { MCPToolCall } from "@/data/MCPToolCall";
import { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { getErrorMessage } from "@/utils/error";
import {
    buildPreviewCodeSignature,
    parsePreviewCodeRequestLoose,
    parsePreviewCodeToolResult,
    PreviewCodeRequestEvent,
} from "@/utils/previewCode";
import { createPreviewCodeRuntime } from "@/utils/previewCodeRuntime";
import { CheckCircle, Loader2, Sparkles, XCircle } from "lucide-react";

interface InlineCodePreviewCardProps {
    parameters: string;
    llmCallId?: string;
    conversationId?: number;
    messageId?: number;
    callId?: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    isStreaming?: boolean;
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
    mcpToolCallStates,
    isStreaming = false,
}: InlineCodePreviewCardProps) {
    const hostRef = useRef<HTMLDivElement | null>(null);
    const runtimeRef = useRef<ReturnType<typeof createPreviewCodeRuntime> | null>(null);
    const [resolvedRequestId, setResolvedRequestId] = useState<string | null>(null);
    const [persistedToolCall, setPersistedToolCall] = useState<MCPToolCall | null>(null);
    const [runtimeError, setRuntimeError] = useState<string | null>(null);
    const [interactionError, setInteractionError] = useState<string | null>(null);
    const [isSubmitting, setIsSubmitting] = useState(false);
    const [optimisticResult, setOptimisticResult] = useState<ReturnType<typeof parsePreviewCodeToolResult> | null>(null);

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
    const stateOverride =
        (effectiveCallId && mcpToolCallStates
            ? mcpToolCallStates.get(effectiveCallId)
            : undefined) ?? matchedStateByLlmCallId;
    const effectiveParameters = isStreaming
        ? parameters
        : stateOverride?.parameters ?? persistedToolCall?.parameters ?? parameters;
    const previewRequest = useMemo(
        () => parsePreviewCodeRequestLoose(effectiveParameters),
        [effectiveParameters]
    );
    const requestSignature = useMemo(
        () =>
            buildPreviewCodeSignature(
                previewRequest
                    ? {
                          title: previewRequest.title,
                          renderer: previewRequest.renderer,
                          code: previewRequest.code,
                          interactionMode: previewRequest.interactionMode,
                      }
                    : null
            ),
        [previewRequest]
    );

    useEffect(() => {
        setResolvedRequestId(null);
        setOptimisticResult(null);
        setInteractionError(null);
        setRuntimeError(null);
    }, [conversationId, requestSignature]);

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
        if (!hostRef.current || runtimeRef.current) {
            return;
        }
        runtimeRef.current = createPreviewCodeRuntime(hostRef.current);
        return () => {
            runtimeRef.current?.destroy();
            runtimeRef.current = null;
        };
    }, []);

    useEffect(() => {
        if (!previewRequest || !runtimeRef.current) {
            return;
        }

        const bridgeId =
            `preview-code-${effectiveCallId ?? llmCallId ?? messageId ?? "transient"}`;
        runtimeRef.current.update({
            code: previewRequest.code,
            isFinal: !isStreaming,
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
                emitEvent: (name: string, payload?: unknown) => {
                    console.debug("[preview_code:event]", { name, payload, bridgeId });
                },
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
    ]);

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

    const handleCloseClick = async () => {
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
    };

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
                return (
                    <Badge variant="secondary" className="flex items-center gap-1">
                        <Loader2 className="h-3 w-3 animate-spin" />
                        等待交互
                    </Badge>
                );
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

    const contentHidden = displayState === "dismissed";

    return (
        <Card className="border-border/80">
            <CardHeader className="space-y-3">
                <div className="flex items-start justify-between gap-3">
                    <div className="space-y-1">
                        <CardTitle className="text-sm">
                            {previewRequest?.title || "inline_preview"}
                        </CardTitle>
                        <div className="text-xs text-muted-foreground">
                            preview_code · {previewRequest?.renderer || "html"}
                        </div>
                    </div>
                    <div className="flex items-center gap-2">
                        {statusBadge}
                        {displayState !== "submitted" && (
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                onClick={handleCloseClick}
                                disabled={isSubmitting || !resolvedRequestId}
                            >
                                关闭
                            </Button>
                        )}
                    </div>
                </div>
                {loadingMessage && (displayState === "streaming" || displayState === "executing" || displayState === "pending") && (
                    <div className="text-xs text-muted-foreground">{loadingMessage}</div>
                )}
            </CardHeader>
            <CardContent className="space-y-3">
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
                {!contentHidden && (
                    <div
                        ref={hostRef}
                        data-testid="preview-code-host"
                        className="rounded-md border bg-background min-h-[96px] p-4 overflow-hidden"
                    />
                )}
                {toolResult?.status === "submitted" && toolResult.payload !== undefined && (
                    <pre className="rounded-md bg-muted p-3 text-xs whitespace-pre-wrap break-words">
                        {JSON.stringify(toolResult.payload, null, 2)}
                    </pre>
                )}
                {contentHidden && (
                    <div className="rounded-md border border-dashed px-3 py-4 text-sm text-muted-foreground">
                        该内嵌 UI 已关闭。
                    </div>
                )}
            </CardContent>
        </Card>
    );
}

