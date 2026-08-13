import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { MCPToolCall } from "@/data/MCPToolCall";
import { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { getErrorMessage } from "@/utils/error";
import { MotionDetails, MotionStatusSlot } from "@/components/mcp-tool-components/McpToolMotion";
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
import PreviewExternalResourcesDialog from "@/components/PreviewExternalResourcesDialog";
import {
    hasActionablePreviewResources,
    PreviewExternalResource,
    PreviewExternalResourcesPayload,
    PreviewExternalResourceType,
    PreviewResourceAuthorizationResult,
} from "@/utils/previewExternalResources";

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

function hasRawExternalPreviewResources(code?: string) {
    if (!code) {
        return false;
    }
    return /<(img|script|link|video|audio|source|iframe|object|embed|image)\b[^>]*(src|href|data|srcset|xlink:href)\s*=\s*["']?\s*(https?:\/\/|file:|\/)/i.test(code)
        || /(?:url\(|@import\s+(?:url\()?)\s*["']?\s*(https?:\/\/|file:|\/)/i.test(code);
}

function isRawExternalPreviewUrl(rawUrl: string) {
    return /^(https?:\/\/|file:|\/)/i.test(rawUrl.trim());
}

function getClientPreviewPlaceholder(type: PreviewExternalResourceType) {
    switch (type) {
        case "image":
            return "data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22/%3E";
        case "css":
            return "data:text/css,";
        case "script":
            return "";
        case "font":
            return "data:font/woff2;base64,";
        case "pdf":
        case "html":
            return "about:blank";
        case "media":
        case "text":
        case "markdown":
        case "unknown":
        default:
            return "";
    }
}

function rewriteClientPreviewCss(css: string) {
    return css
        .replace(/url\(\s*(?:"([^"]*)"|'([^']*)'|([^'")\s][^)]*?))\s*\)/gi, (_match, doubleQuoted, singleQuoted, unquoted) => {
            const rawUrl = String(doubleQuoted ?? singleQuoted ?? unquoted ?? "").trim();
            if (!rawUrl || !isRawExternalPreviewUrl(rawUrl)) {
                return _match;
            }
            return 'url("data:image/svg+xml,%3Csvg xmlns=%22http://www.w3.org/2000/svg%22/%3E")';
        })
        .replace(/@import\s+(?:url\(\s*)?(?:"([^"]*)"|'([^']*)'|([^\s"'()]+))\s*\)?/gi, (_match, doubleQuoted, singleQuoted, unquoted) => {
            const rawUrl = String(doubleQuoted ?? singleQuoted ?? unquoted ?? "").trim();
            if (!rawUrl || !isRawExternalPreviewUrl(rawUrl)) {
                return _match;
            }
            return '@import url("data:text/css,")';
        });
}

function sanitizeClientPreviewCode(code: string) {
    const template = document.createElement("template");
    template.innerHTML = code;

    template.content.querySelectorAll("style").forEach((style) => {
        style.textContent = rewriteClientPreviewCss(style.textContent ?? "");
    });

    template.content.querySelectorAll("[style]").forEach((element) => {
        const styleValue = element.getAttribute("style");
        if (!styleValue) {
            return;
        }
        element.setAttribute("style", rewriteClientPreviewCss(styleValue));
    });

    template.content.querySelectorAll("img, image, script, link, video, audio, source, iframe, object, embed").forEach((element) => {
        const tagName = element.tagName.toLowerCase();
        const resourceType: PreviewExternalResourceType =
            tagName === "img" || tagName === "image"
                ? "image"
                : tagName === "script"
                  ? "script"
                  : tagName === "link"
                    ? "css"
                    : tagName === "iframe" || tagName === "object" || tagName === "embed"
                      ? "html"
                      : tagName === "video" || tagName === "audio" || tagName === "source"
                        ? "media"
                        : "unknown";
        const placeholder = getClientPreviewPlaceholder(resourceType);
        for (const attrName of ["src", "href", "data"]) {
            const rawValue = element.getAttribute(attrName);
            if (!rawValue || !isRawExternalPreviewUrl(rawValue)) {
                continue;
            }
            if (resourceType === "script" && attrName === "src") {
                element.removeAttribute(attrName);
                continue;
            }
            element.setAttribute(attrName, placeholder);
        }
        const srcset = element.getAttribute("srcset");
        if (srcset) {
            const rewritten = srcset
                .split(",")
                .map((entry) => {
                    const trimmed = entry.trim();
                    if (!trimmed) {
                        return trimmed;
                    }
                    const [rawUrl, ...descriptorParts] = trimmed.split(/\s+/);
                    if (!rawUrl || !isRawExternalPreviewUrl(rawUrl)) {
                        return trimmed;
                    }
                    const descriptor = descriptorParts.join(" ");
                    return descriptor ? `${getClientPreviewPlaceholder("image")} ${descriptor}` : getClientPreviewPlaceholder("image");
                })
                .filter(Boolean)
                .join(", ");
            element.setAttribute("srcset", rewritten);
        }
    });

    return template.innerHTML;
}

function normalizePreviewResourceUrl(rawUrl: string) {
    try {
        return new URL(rawUrl).toString();
    } catch {
        return rawUrl;
    }
}

function inferPreviewResourceType(
    rawUrl: string,
    fallback: PreviewExternalResourceType
): PreviewExternalResourceType {
    const path = rawUrl.split(/[?#]/, 1)[0]?.toLowerCase() ?? "";
    if (/\.(png|jpe?g|gif|webp|avif|svg|bmp|ico)$/.test(path)) {
        return "image";
    }
    if (/\.(css)$/.test(path)) {
        return "css";
    }
    if (/\.(js|mjs|cjs)$/.test(path)) {
        return "script";
    }
    if (/\.(woff2?|ttf|otf|eot)$/.test(path)) {
        return "font";
    }
    if (/\.(mp4|webm|ogg|mp3|wav|m4a)$/.test(path)) {
        return "media";
    }
    if (/\.(pdf)$/.test(path)) {
        return "pdf";
    }
    if (/\.(html?|xhtml)$/.test(path)) {
        return "html";
    }
    return fallback;
}

function buildClientDetectedExternalResources(code?: string): PreviewExternalResourcesPayload | null {
    if (!code) {
        return null;
    }
    const candidates: Array<{
        url: string;
        type: PreviewExternalResourceType;
        occurrence: string;
    }> = [];
    const pushCandidate = (
        url: string,
        type: PreviewExternalResourceType,
        occurrence: string
    ) => {
        const trimmed = url.trim();
        if (!/^(https?:\/\/|file:|\/)/i.test(trimmed)) {
            return;
        }
        candidates.push({
            url: trimmed,
            type: inferPreviewResourceType(trimmed, type),
            occurrence,
        });
    };

    const tagRegex = /<(img|script|link|video|audio|source|iframe|object|embed|image)\b[^>]*>/gi;
    const attrRegex = /\b(srcset|src|href|data|xlink:href)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'<>`]+))/gi;
    let tagMatch: RegExpExecArray | null;
    while ((tagMatch = tagRegex.exec(code)) !== null) {
        const tag = tagMatch[0];
        const tagName = tagMatch[1].toLowerCase();
        const lowerTag = tag.toLowerCase();
        const fallback: PreviewExternalResourceType =
            tagName === "img" || tagName === "image"
                ? "image"
                : tagName === "script"
                  ? "script"
                  : tagName === "link" && lowerTag.includes("stylesheet")
                    ? "css"
                    : tagName === "video" || tagName === "audio" || tagName === "source"
                      ? "media"
                      : tagName === "iframe" || tagName === "object" || tagName === "embed"
                        ? "html"
                        : "unknown";
        if (fallback === "unknown") {
            continue;
        }
        attrRegex.lastIndex = 0;
        let attrMatch: RegExpExecArray | null;
        while ((attrMatch = attrRegex.exec(tag)) !== null) {
            const attrName = attrMatch[1].toLowerCase();
            const rawValue = attrMatch[2] ?? attrMatch[3] ?? attrMatch[4];
            if (!rawValue) {
                continue;
            }
            if (attrName === "srcset") {
                rawValue.split(",").forEach((entry) => {
                    const [url] = entry.trim().split(/\s+/);
                    if (url) {
                        pushCandidate(url, fallback, `${tagName} srcset`);
                    }
                });
            } else {
                pushCandidate(rawValue, fallback, `${tagName} ${attrName}`);
            }
        }
    }

    const cssUrlRegex = /url\(\s*(['"]?)([^'")]+)\1\s*\)/gi;
    const cssImportRegex = /@import\s+(?:url\(\s*)?(?:"([^"]*)"|'([^']*)'|([^\s"'()]+))\s*\)?/gi;
    let cssMatch: RegExpExecArray | null;
    while ((cssMatch = cssUrlRegex.exec(code)) !== null) {
        pushCandidate(cssMatch[2], "image", "css url");
    }
    while ((cssMatch = cssImportRegex.exec(code)) !== null) {
        const rawUrl = cssMatch[2] ?? cssMatch[3] ?? cssMatch[4];
        if (rawUrl) {
            pushCandidate(rawUrl, "css", "css import");
        }
    }

    const seen = new Set<string>();
    const resources = candidates
        .filter((candidate) => {
            const key = `${candidate.type}\n${candidate.url}`;
            if (seen.has(key)) {
                return false;
            }
            seen.add(key);
            return true;
        })
        .map((candidate, index) => ({
            id: `client-detected-${index}`,
            originalUrl: candidate.url,
            normalizedUrl: normalizePreviewResourceUrl(candidate.url),
            type: candidate.type,
            source: "preview_code",
            occurrence: candidate.occurrence,
            status: "pending" as const,
            risk:
                candidate.type === "script" || candidate.type === "html"
                    ? ("high" as const)
                    : candidate.type === "css"
                      ? ("medium" as const)
                      : ("low" as const),
        }));

    return resources.length > 0
        ? {
              requestId: "client-detected-preview-resources",
              resources,
          }
        : null;
}

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
    const [preparedPreviewRequest, setPreparedPreviewRequest] =
        useState<PreviewCodeRequestEvent | null>(null);
    const [clientDetectedExternalResources, setClientDetectedExternalResources] =
        useState<PreviewExternalResourcesPayload | null>(null);
    const [isResourceDialogOpen, setIsResourceDialogOpen] = useState(false);
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
    const preparingRequestSignatureRef = useRef<string | null>(null);
    const reportedRuntimeErrorKeysRef = useRef<Set<string>>(new Set());

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

    const rawPreviewRequest = useMemo(() => {
        if (isStreaming && activeStreamingPreviewState) {
            return activeStreamingPreviewState;
        }
        return parsePreviewCodeRequestLoose(effectiveParameters);
    }, [activeStreamingPreviewState, effectiveParameters, isStreaming]);
    const previewRequest = preparedPreviewRequest ?? rawPreviewRequest;
    const requestSignature = useMemo(
        () =>
            isStreaming
                ? null
                : buildPreviewCodeSignature(
                          rawPreviewRequest
                          ? {
                                title: rawPreviewRequest.title,
                                renderer: rawPreviewRequest.renderer,
                                code: rawPreviewRequest.code,
                                interactionMode: rawPreviewRequest.interactionMode,
                            }
                          : null
                  ),
        [isStreaming, rawPreviewRequest]
    );
    const rawPreviewTitle = rawPreviewRequest?.title ?? null;
    const rawPreviewRenderer = rawPreviewRequest?.renderer ?? null;
    const rawPreviewInteractionMode = rawPreviewRequest?.interactionMode ?? null;
    const rawPreviewCode = rawPreviewRequest?.code ?? null;
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
        setPreparedPreviewRequest(null);
        setClientDetectedExternalResources(null);
        setOptimisticResult(null);
        setInteractionError(null);
        setRuntimeError(null);
        setIsCollapsed(defaultCollapsed);
        setIsHidden(false);
        setIsRuntimeActivated(isStreaming || !defaultCollapsed);
        preparingRequestSignatureRef.current = null;
        reportedRuntimeErrorKeysRef.current.clear();
    }, [conversationId, hasScriptContent, isLastMessage, isStreaming, requestSignature]);

    const handleRuntimeError = useCallback(
        (message: string | null) => {
            setRuntimeError(message);
            if (!message || !effectiveCallId || isStreaming) {
                return;
            }
            if (!resolvedRequestId) {
                return;
            }

            const reportKey = `${effectiveCallId}:${requestSignature ?? previewIdentity}:${message}`;
            if (reportedRuntimeErrorKeysRef.current.has(reportKey)) {
                return;
            }
            reportedRuntimeErrorKeysRef.current.add(reportKey);

            const code = previewRequest?.code?.trim() ?? "";
            void invoke("report_preview_code_runtime_error", {
                callId: effectiveCallId,
                call_id: effectiveCallId,
                conversationId,
                conversation_id: conversationId,
                messageId,
                message_id: messageId,
                requestId: resolvedRequestId,
                request_id: resolvedRequestId,
                error: {
                    message,
                    phase: "runtime",
                    codeExcerpt: code.length > 2000 ? `${code.slice(0, 2000)}...` : code,
                },
            }).catch((error) => {
                console.warn("Failed to report preview_code runtime error:", getErrorMessage(error));
            });
        },
        [
            conversationId,
            effectiveCallId,
            isStreaming,
            messageId,
            previewIdentity,
            previewRequest?.code,
            requestSignature,
            resolvedRequestId,
        ]
    );

    useEffect(() => {
        if (runtimeError && resolvedRequestId) {
            handleRuntimeError(runtimeError);
        }
    }, [handleRuntimeError, resolvedRequestId, runtimeError]);

    const authorizeSelectedPreviewResources = useCallback(
        async (
            selectedResourceIds: string[],
            options: { addToWhitelist: boolean; useProxy: boolean }
        ) => {
            let resourcesPayload = previewRequest?.externalResources ?? null;
            let resourceIds = selectedResourceIds;

            if (!resourcesPayload || resourcesPayload.requestId === "client-detected-preview-resources") {
                const selectedClientResources =
                    clientDetectedExternalResources?.resources.filter((resource) =>
                        selectedResourceIds.includes(resource.id)
                    ) ?? [];
                if (!rawPreviewRequest || selectedClientResources.length === 0) {
                    throw new Error("没有可加载的外部资源。");
                }
                return invoke<PreviewResourceAuthorizationResult<PreviewCodeRequestEvent, unknown>>(
                    "authorize_preview_code_external_resource_urls",
                    {
                        conversationId,
                        conversation_id: conversationId,
                        request: rawPreviewRequest,
                        resources: selectedClientResources.map((resource: PreviewExternalResource) => ({
                            originalUrl: resource.originalUrl,
                            original_url: resource.originalUrl,
                            normalizedUrl: resource.normalizedUrl,
                            normalized_url: resource.normalizedUrl,
                            type: resource.type,
                            occurrence: resource.occurrence,
                        })),
                        addToWhitelist: options.addToWhitelist,
                        add_to_whitelist: options.addToWhitelist,
                        useProxy: options.useProxy,
                        use_proxy: options.useProxy,
                    }
                );
            }

            return invoke<PreviewResourceAuthorizationResult<PreviewCodeRequestEvent, unknown>>(
                "authorize_preview_external_resources",
                {
                    requestId: resourcesPayload.requestId,
                    request_id: resourcesPayload.requestId,
                    resourceIds,
                    resource_ids: resourceIds,
                    addToWhitelist: options.addToWhitelist,
                    add_to_whitelist: options.addToWhitelist,
                    useProxy: options.useProxy,
                    use_proxy: options.useProxy,
                }
            );
        },
        [
            clientDetectedExternalResources,
            conversationId,
            previewRequest?.externalResources,
            rawPreviewRequest,
        ]
    );

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
                    return signature === requestSignature
                        || (
                            rawPreviewRequest
                            && event.title === rawPreviewTitle
                            && event.renderer === rawPreviewRenderer
                            && event.interactionMode === rawPreviewInteractionMode
                        );
                });
                if (matched) {
                    setResolvedRequestId(matched.request_id);
                    setPreparedPreviewRequest(matched);
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
    }, [
        conversationId,
        requestSignature,
        rawPreviewInteractionMode,
        rawPreviewRenderer,
        rawPreviewTitle,
    ]);

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
            if (
                signature === requestSignature
                || (
                    rawPreviewRequest
                    && payload.title === rawPreviewTitle
                    && payload.renderer === rawPreviewRenderer
                    && payload.interactionMode === rawPreviewInteractionMode
                )
            ) {
                setResolvedRequestId(payload.request_id);
                setPreparedPreviewRequest(payload);
            }
        });

        return () => {
            void unsubscribe.then((dispose) => dispose()).catch(() => undefined);
        };
    }, [
        conversationId,
        requestSignature,
        rawPreviewInteractionMode,
        rawPreviewRenderer,
        rawPreviewTitle,
    ]);

    useEffect(() => {
        if (
            isStreaming
            || !rawPreviewRequest
            || !requestSignature
            || preparedPreviewRequest
            || !hasRawExternalPreviewResources(rawPreviewCode ?? undefined)
        ) {
            return;
        }
        if (preparingRequestSignatureRef.current === requestSignature) {
            return;
        }

        let active = true;
        preparingRequestSignatureRef.current = requestSignature;
        invoke<PreviewCodeRequestEvent>("prepare_preview_code_request_for_ui", {
            conversationId,
            conversation_id: conversationId,
            request: rawPreviewRequest,
        })
            .then((payload) => {
                if (!active || preparingRequestSignatureRef.current !== requestSignature) {
                    return;
                }
                setResolvedRequestId(payload.request_id);
                setPreparedPreviewRequest(payload);
                setClientDetectedExternalResources(null);
            })
            .catch((error) => {
                if (!active || preparingRequestSignatureRef.current !== requestSignature) {
                    return;
                }
                setRuntimeError(`preview_code 外部资源准备失败: ${getErrorMessage(error)}`);
            });

        return () => {
            active = false;
        };
    }, [
        conversationId,
        isStreaming,
        preparedPreviewRequest,
        rawPreviewCode,
        rawPreviewInteractionMode,
        rawPreviewRenderer,
        rawPreviewTitle,
        requestSignature,
    ]);

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
            code:
                previewRequest === rawPreviewRequest && hasRawExternalPreviewResources(rawPreviewRequest?.code)
                    ? sanitizeClientPreviewCode(renderCode)
                    : renderCode,
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
            onError: handleRuntimeError,
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
            handleRuntimeError,
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
    const hasPreparedExternalResources = Boolean(previewRequest?.externalResources);
    const showExternalResourceButton =
        hasActionablePreviewResources(previewRequest?.externalResources)
        || (!hasPreparedExternalResources && hasRawExternalPreviewResources(rawPreviewRequest?.code));

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

    const submittedPayload = toolResult?.status === "submitted" ? toolResult.payload : undefined;
    const hasSubmittedPayload = submittedPayload !== undefined;

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
                            <MotionStatusSlot stateKey={displayState} present={statusBadge !== null}>
                                {statusBadge}
                            </MotionStatusSlot>
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
                    <MotionDetails
                        show={Boolean(loadingMessage) && (displayState === "streaming" || displayState === "executing" || displayState === "pending")}
                    >
                        <div className="text-xs text-muted-foreground">{loadingMessage}</div>
                    </MotionDetails>
                </div>
            )}
            <div className="space-y-3">
                {showExternalResourceButton && (
                    <div className="flex justify-end">
                        <Button
                            type="button"
                            variant="outline"
                            size="sm"
                            onClick={() => {
                                if (!hasActionablePreviewResources(previewRequest?.externalResources)) {
                                    setClientDetectedExternalResources(
                                        buildClientDetectedExternalResources(rawPreviewRequest?.code)
                                    );
                                }
                                setIsResourceDialogOpen(true);
                            }}
                        >
                            需要加载外部资源
                        </Button>
                    </div>
                )}
                <MotionDetails show={Boolean(runtimeError)}>
                    <div className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                        {runtimeError}
                    </div>
                </MotionDetails>
                <MotionDetails show={Boolean(interactionError)}>
                    <div className="rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive">
                        {interactionError}
                    </div>
                </MotionDetails>
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
                <MotionDetails show={shouldShowStreamingFallback && !previewHidden && Boolean(sourceExcerpt)}>
                    <div className="rounded-md border border-border/70 bg-muted/40 px-3 py-3 space-y-2">
                        <div className="text-xs text-muted-foreground">
                            正在生成可渲染预览，先展示当前代码片段。
                        </div>
                        <pre className="text-xs whitespace-pre-wrap break-words text-foreground/80">
                            {sourceExcerpt}
                        </pre>
                    </div>
                </MotionDetails>
                <MotionDetails show={hasSubmittedPayload}>
                    <pre className="rounded-md bg-muted p-3 text-xs whitespace-pre-wrap break-words">
                        {JSON.stringify(submittedPayload, null, 2)}
                    </pre>
                </MotionDetails>
                <MotionDetails show={isHidden && displayState !== "dismissed"}>
                    <div className="text-sm text-muted-foreground">
                        预览已隐藏。
                    </div>
                </MotionDetails>
                <MotionDetails show={displayState === "dismissed"}>
                    <div className="text-sm text-muted-foreground">
                        该内嵌 UI 已关闭。
                    </div>
                </MotionDetails>
            </div>
            <PreviewExternalResourcesDialog<PreviewCodeRequestEvent, unknown>
                externalResources={previewRequest?.externalResources ?? clientDetectedExternalResources}
                open={isResourceDialogOpen}
                onOpenChange={setIsResourceDialogOpen}
                canAuthorize={true}
                onAuthorizeSelected={authorizeSelectedPreviewResources}
                onAuthorized={(result: PreviewResourceAuthorizationResult<PreviewCodeRequestEvent, unknown>) => {
                    if (result.previewCode) {
                        setPreparedPreviewRequest(result.previewCode);
                        setClientDetectedExternalResources(null);
                        setResolvedRequestId(result.previewCode.request_id);
                    }
                }}
            />
        </div>
    );
}
