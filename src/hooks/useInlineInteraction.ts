import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { AskUserQuestionRequest, PreviewFileRequest } from "@/components/InlineInteractionCards";
import { MCPToolCall } from "@/data/MCPToolCall";
import { getErrorMessage } from "@/utils/error";

type AskUserQuestionViewMode = "questionnaire" | "summary";

interface AskUserQuestionPersistedState {
    callId: number;
    messageId: number | null;
    request: AskUserQuestionRequest;
    status: "pending" | "executing" | "success" | "failed";
    answers: Record<string, string> | null;
}

interface PreviewFilePersistedState {
    callId: number;
    messageId: number | null;
    request: PreviewFileRequest;
    status: "pending" | "executing" | "success" | "failed";
    requestId: string | null;
}

const ASK_USER_QUESTION_TOOL_NAME = "ask_user_question";
const PREVIEW_FILE_TOOL_NAME = "preview_file";

function safeParseJson(value: string): unknown {
    try {
        return JSON.parse(value);
    } catch {
        return null;
    }
}

function normalizeQuestions(raw: unknown): AskUserQuestionRequest["questions"] {
    if (!Array.isArray(raw)) return [];
    return raw
        .map((item) => {
            if (!item || typeof item !== "object") return null;
            const record = item as Record<string, unknown>;
            const question = typeof record.question === "string" ? record.question : "";
            const header = typeof record.header === "string" ? record.header : "问题";
            const multiSelect = record.multiSelect === true;
            const optionsRaw = Array.isArray(record.options) ? record.options : [];
            const options = optionsRaw
                .map((option) => {
                    if (!option || typeof option !== "object") return null;
                    const optionRecord = option as Record<string, unknown>;
                    const label = typeof optionRecord.label === "string" ? optionRecord.label : "";
                    const description =
                        typeof optionRecord.description === "string" ? optionRecord.description : "";
                    if (!label) return null;
                    return { label, description };
                })
                .filter((option): option is { label: string; description: string } => option !== null);
            if (!question || options.length === 0) return null;
            return { question, header, options, multiSelect };
        })
        .filter(
            (
                item
            ): item is {
                question: string;
                header: string;
                options: { label: string; description: string }[];
                multiSelect: boolean;
            } => item !== null
        );
}

function parseAskUserQuestionRequest(
    parameters: string,
    conversationId: number | undefined,
    requestId: string
): AskUserQuestionRequest | null {
    const parsed = safeParseJson(parameters);
    if (!parsed || typeof parsed !== "object") return null;
    const record = parsed as Record<string, unknown>;
    const questions = normalizeQuestions(record.questions);
    if (questions.length === 0) return null;
    const metadata =
        record.metadata && typeof record.metadata === "object"
            ? {
                  source:
                      typeof (record.metadata as Record<string, unknown>).source === "string"
                          ? ((record.metadata as Record<string, unknown>).source as string)
                          : undefined,
              }
            : undefined;

    return {
        request_id: requestId,
        conversation_id: conversationId,
        questions,
        metadata,
    };
}

function normalizeAnswers(raw: unknown): Record<string, string> | null {
    if (!raw || typeof raw !== "object") return null;
    const result: Record<string, string> = {};
    Object.entries(raw as Record<string, unknown>).forEach(([key, value]) => {
        if (typeof key !== "string" || !key.trim()) return;
        if (typeof value === "string") {
            result[key] = value;
            return;
        }
        if (value === null || value === undefined) {
            result[key] = "";
            return;
        }
        result[key] = String(value);
    });
    return Object.keys(result).length > 0 ? result : null;
}

function extractAnswersFromResultNode(node: unknown): Record<string, string> | null {
    if (!node) return null;

    if (typeof node === "string") {
        const parsed = safeParseJson(node);
        if (!parsed) return null;
        return extractAnswersFromResultNode(parsed);
    }

    if (Array.isArray(node)) {
        for (const item of node) {
            const extracted = extractAnswersFromResultNode(item);
            if (extracted) return extracted;
        }
        return null;
    }

    if (typeof node === "object") {
        const record = node as Record<string, unknown>;
        if (record.answers) {
            const normalized = normalizeAnswers(record.answers);
            if (normalized) return normalized;
        }
        if (record.json) {
            const fromJson = extractAnswersFromResultNode(record.json);
            if (fromJson) return fromJson;
        }
        if (record.content) {
            const fromContent = extractAnswersFromResultNode(record.content);
            if (fromContent) return fromContent;
        }
    }

    return null;
}

function parseAskUserQuestionAnswers(result?: string): Record<string, string> | null {
    if (!result?.trim()) return null;
    const parsed = safeParseJson(result);
    if (!parsed) return null;
    return extractAnswersFromResultNode(parsed);
}

function buildAskUserQuestionSignature(request: AskUserQuestionRequest | null): string | null {
    if (!request) return null;
    return JSON.stringify(
        request.questions.map((question) => ({
            header: question.header,
            question: question.question,
            multiSelect: question.multiSelect,
            options: question.options.map((option) => ({
                label: option.label,
                description: option.description,
            })),
        }))
    );
}

function parsePersistedAskCallId(requestId: string): number | null {
    const prefix = "persisted-ask-";
    if (!requestId.startsWith(prefix)) {
        return null;
    }
    const raw = requestId.slice(prefix.length);
    const parsed = Number.parseInt(raw, 10);
    if (!Number.isFinite(parsed) || parsed <= 0) {
        return null;
    }
    return parsed;
}

function normalizePreviewFiles(raw: unknown): PreviewFileRequest["files"] {
    if (!Array.isArray(raw)) return [];
    const files: PreviewFileRequest["files"] = [];
    raw.forEach((item) => {
        if (!item || typeof item !== "object") return;
        const record = item as Record<string, unknown>;
        const title = typeof record.title === "string" ? record.title : "";
        const type = typeof record.type === "string" ? record.type : "";
        if (!title || !type) return;
        files.push({
            title,
            type,
            content: typeof record.content === "string" ? record.content : undefined,
            url: typeof record.url === "string" ? record.url : undefined,
            language: typeof record.language === "string" ? record.language : undefined,
            description:
                typeof record.description === "string" ? record.description : undefined,
        });
    });
    return files;
}

function parsePreviewFileRequest(
    parameters: string,
    conversationId: number | undefined,
    requestId: string
): PreviewFileRequest | null {
    const parsed = safeParseJson(parameters);
    if (!parsed || typeof parsed !== "object") return null;
    const record = parsed as Record<string, unknown>;
    const files = normalizePreviewFiles(record.files);
    if (files.length === 0) return null;

    const rawViewMode =
        typeof record.viewMode === "string"
            ? record.viewMode
            : typeof record.view_mode === "string"
              ? record.view_mode
              : undefined;
    const viewMode =
        rawViewMode === "tabs" || rawViewMode === "list" || rawViewMode === "grid"
            ? rawViewMode
            : undefined;
    const metadata =
        record.metadata && typeof record.metadata === "object"
            ? {
                  origin:
                      typeof (record.metadata as Record<string, unknown>).origin === "string"
                          ? ((record.metadata as Record<string, unknown>).origin as string)
                          : undefined,
              }
            : undefined;

    return {
        request_id: requestId,
        conversation_id: conversationId,
        files,
        viewMode,
        metadata,
    };
}

function extractPreviewRequestIdFromNode(node: unknown): string | null {
    if (!node) return null;

    if (typeof node === "string") {
        const parsed = safeParseJson(node);
        if (!parsed) return null;
        return extractPreviewRequestIdFromNode(parsed);
    }

    if (Array.isArray(node)) {
        for (const item of node) {
            const requestId = extractPreviewRequestIdFromNode(item);
            if (requestId) return requestId;
        }
        return null;
    }

    if (typeof node === "object") {
        const record = node as Record<string, unknown>;
        if (typeof record.request_id === "string" && record.request_id.trim()) {
            return record.request_id;
        }
        if (record.json) {
            const fromJson = extractPreviewRequestIdFromNode(record.json);
            if (fromJson) return fromJson;
        }
        if (record.content) {
            const fromContent = extractPreviewRequestIdFromNode(record.content);
            if (fromContent) return fromContent;
        }
        if (record.result) {
            const fromResult = extractPreviewRequestIdFromNode(record.result);
            if (fromResult) return fromResult;
        }
    }

    return null;
}

function parsePreviewFileRequestId(result?: string): string | null {
    if (!result?.trim()) return null;
    const parsed = safeParseJson(result);
    if (!parsed) return null;
    return extractPreviewRequestIdFromNode(parsed);
}

function buildPreviewFileSignature(request: PreviewFileRequest | null): string | null {
    if (!request) return null;
    return JSON.stringify({
        viewMode: request.viewMode ?? "tabs",
        files: request.files.map((file) => ({
            title: file.title,
            type: file.type,
            content: file.content ?? null,
            url: file.url ?? null,
            language: file.language ?? null,
            description: file.description ?? null,
        })),
    });
}

function getLatestAskUserQuestionState(
    calls: MCPToolCall[],
    conversationId: number | undefined
): AskUserQuestionPersistedState | null {
    const candidates = calls
        .filter((call) => call.tool_name === ASK_USER_QUESTION_TOOL_NAME)
        .sort((a, b) => b.id - a.id);

    for (const call of candidates) {
        if (!["pending", "executing", "success", "failed"].includes(call.status)) {
            continue;
        }
        const request = parseAskUserQuestionRequest(
            call.parameters,
            conversationId,
            `persisted-ask-${call.id}`
        );
        if (!request) continue;
        return {
            callId: call.id,
            messageId: call.message_id ?? null,
            request,
            status: call.status,
            answers: call.status === "success" ? parseAskUserQuestionAnswers(call.result) : null,
        };
    }

    return null;
}

function getLatestPreviewFileState(
    calls: MCPToolCall[],
    conversationId: number | undefined
): PreviewFilePersistedState | null {
    const candidates = calls
        .filter((call) => call.tool_name === PREVIEW_FILE_TOOL_NAME)
        .sort((a, b) => b.id - a.id);

    for (const call of candidates) {
        if (!["pending", "executing", "success", "failed"].includes(call.status)) {
            continue;
        }
        const request = parsePreviewFileRequest(
            call.parameters,
            conversationId,
            `persisted-preview-${call.id}`
        );
        if (!request) continue;
        return {
            callId: call.id,
            messageId: call.message_id ?? null,
            request,
            status: call.status,
            requestId: parsePreviewFileRequestId(call.result),
        };
    }

    return null;
}

interface UseAskUserQuestionOptions {
    conversationId?: number;
}

export function useAskUserQuestion(options: UseAskUserQuestionOptions = {}) {
    const { conversationId } = options;
    const [pendingRequest, setPendingRequest] = useState<AskUserQuestionRequest | null>(null);
    const [, setRequestQueue] = useState<AskUserQuestionRequest[]>([]);
    const [isDialogOpen, setIsDialogOpen] = useState(false);
    const [persistedState, setPersistedState] = useState<AskUserQuestionPersistedState | null>(null);
    const [submittedSummary, setSubmittedSummary] = useState<{
        request: AskUserQuestionRequest;
        answers: Record<string, string>;
    } | null>(null);

    const refreshPersistedState = useCallback(async () => {
        if (conversationId === undefined) {
            setPersistedState(null);
            return;
        }
        try {
            const calls = await invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", {
                conversationId,
            });
            setPersistedState(getLatestAskUserQuestionState(calls, conversationId));
        } catch (error) {
            console.error("Failed to load persisted ask_user_question state:", getErrorMessage(error));
        }
    }, [conversationId]);

    const shiftNextRequest = useCallback(() => {
        setRequestQueue((prev) => {
            const [, ...rest] = prev;
            setPendingRequest(rest[0] ?? null);
            setIsDialogOpen(rest.length > 0);
            return rest;
        });
    }, []);

    useEffect(() => {
        setPendingRequest(null);
        setRequestQueue([]);
        setIsDialogOpen(false);
        setSubmittedSummary(null);
    }, [conversationId]);

    useEffect(() => {
        void refreshPersistedState();
    }, [refreshPersistedState]);

    useEffect(() => {
        const unsubscribe = listen<AskUserQuestionRequest>("ask-user-question-request", (event) => {
            const request = event.payload;

            if (
                conversationId !== undefined &&
                request.conversation_id !== undefined &&
                request.conversation_id !== null &&
                request.conversation_id !== conversationId
            ) {
                return;
            }

            setSubmittedSummary(null);
            setRequestQueue((prev) => {
                const next = [...prev, request];
                if (next.length === 1) {
                    setPendingRequest(request);
                    setIsDialogOpen(true);
                }
                return next;
            });
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId]);

    useEffect(() => {
        if (conversationId === undefined) {
            return;
        }

        const eventName = `conversation_event_${conversationId}`;
        const unsubscribe = listen<any>(eventName, (event) => {
            const payload = event.payload as { type?: string; data?: { tool_name?: string } };
            if (payload?.type !== "mcp_tool_call_update") {
                return;
            }
            if (
                payload.data?.tool_name &&
                payload.data.tool_name !== ASK_USER_QUESTION_TOOL_NAME
            ) {
                return;
            }
            void refreshPersistedState();
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId, refreshPersistedState]);

    const schedulePersistedRefresh = useCallback(() => {
        void refreshPersistedState();
        setTimeout(() => {
            void refreshPersistedState();
        }, 300);
        setTimeout(() => {
            void refreshPersistedState();
        }, 1000);
    }, [refreshPersistedState]);

    const handleSubmit = useCallback(
        async (requestId: string, answers: Record<string, string>) => {
            const currentPending = pendingRequest;
            if (!currentPending || currentPending.request_id !== requestId) {
                return;
            }
            setSubmittedSummary({ request: currentPending, answers });
            try {
                await invoke("submit_ask_user_question_response", {
                    requestId,
                    answers,
                    cancelled: false,
                });
            } catch (error) {
                console.error("Failed to submit AskUserQuestion response:", getErrorMessage(error));
            } finally {
                shiftNextRequest();
                schedulePersistedRefresh();
            }
        },
        [pendingRequest, schedulePersistedRefresh, shiftNextRequest]
    );

    const handleCancel = useCallback(
        async (requestId: string) => {
            const persistedCallId = parsePersistedAskCallId(requestId);
            const currentPending = pendingRequest;
            const isRealtimePendingMatch =
                !!currentPending && currentPending.request_id === requestId;
            if (persistedCallId === null && !isRealtimePendingMatch) {
                return;
            }
            const callId = persistedCallId ?? persistedState?.callId ?? null;
            if (callId !== null) {
                try {
                    await invoke("stop_mcp_tool_call", { callId });
                } catch (error) {
                    console.error(
                        "Failed to stop AskUserQuestion MCP tool call:",
                        getErrorMessage(error)
                    );
                }
            }
            try {
                if (persistedCallId === null && isRealtimePendingMatch) {
                    await invoke("submit_ask_user_question_response", {
                        requestId,
                        answers: null,
                        cancelled: true,
                    });
                }
            } catch (error) {
                console.error("Failed to cancel AskUserQuestion response:", getErrorMessage(error));
            } finally {
                if (isRealtimePendingMatch) {
                    shiftNextRequest();
                } else {
                    setPendingRequest(null);
                    setIsDialogOpen(false);
                }
                schedulePersistedRefresh();
            }
        },
        [pendingRequest, persistedState?.callId, schedulePersistedRefresh, shiftNextRequest]
    );

    const askUserQuestionView = useMemo(() => {
        const resolvePersistedAnchor = (
            request: AskUserQuestionRequest | null
        ): AskUserQuestionPersistedState | null => {
            if (!request || !persistedState) {
                return null;
            }
            const requestSignature = buildAskUserQuestionSignature(request);
            const persistedSignature = buildAskUserQuestionSignature(persistedState.request);
            if (!requestSignature || !persistedSignature) {
                return null;
            }
            return requestSignature === persistedSignature ? persistedState : null;
        };

        if (pendingRequest && isDialogOpen) {
            const persistedAnchor = resolvePersistedAnchor(pendingRequest);
            return {
                request: pendingRequest,
                isOpen: true,
                mode: "questionnaire" as AskUserQuestionViewMode,
                completedAnswers: null as Record<string, string> | null,
                readOnly: false,
                callId: persistedAnchor?.callId ?? (null as number | null),
                messageId: persistedAnchor?.messageId ?? (null as number | null),
            };
        }

        if (submittedSummary) {
            const persistedAnchor = resolvePersistedAnchor(submittedSummary.request);
            return {
                request: submittedSummary.request,
                isOpen: true,
                mode: "summary" as AskUserQuestionViewMode,
                completedAnswers: submittedSummary.answers,
                readOnly: true,
                callId: persistedAnchor?.callId ?? (null as number | null),
                messageId: persistedAnchor?.messageId ?? (null as number | null),
            };
        }

        if (persistedState) {
            if (persistedState.status === "success") {
                return {
                    request: persistedState.request,
                    isOpen: true,
                    mode: "summary" as AskUserQuestionViewMode,
                    completedAnswers: persistedState.answers ?? {},
                    readOnly: true,
                    callId: persistedState.callId,
                    messageId: persistedState.messageId,
                };
            }
            if (persistedState.status === "pending" || persistedState.status === "executing") {
                return {
                    request: persistedState.request,
                    isOpen: true,
                    mode: "questionnaire" as AskUserQuestionViewMode,
                    completedAnswers: null as Record<string, string> | null,
                    readOnly: false,
                    callId: persistedState.callId,
                    messageId: persistedState.messageId,
                };
            }
        }

        return {
            request: null as AskUserQuestionRequest | null,
            isOpen: false,
            mode: "questionnaire" as AskUserQuestionViewMode,
            completedAnswers: null as Record<string, string> | null,
            readOnly: false,
            callId: null as number | null,
            messageId: null as number | null,
        };
    }, [pendingRequest, isDialogOpen, persistedState, submittedSummary]);

    return {
        pendingRequest: askUserQuestionView.request,
        isDialogOpen: askUserQuestionView.isOpen,
        viewMode: askUserQuestionView.mode,
        completedAnswers: askUserQuestionView.completedAnswers,
        readOnly: askUserQuestionView.readOnly,
        callId: askUserQuestionView.callId,
        messageId: askUserQuestionView.messageId,
        handleSubmit,
        handleCancel,
    };
}

interface UsePreviewFileOptions {
    conversationId?: number;
}

export function usePreviewFile(options: UsePreviewFileOptions = {}) {
    const { conversationId } = options;
    const [pendingRequest, setPendingRequest] = useState<PreviewFileRequest | null>(null);
    const [isDialogOpen, setIsDialogOpen] = useState(false);
    const [persistedState, setPersistedState] = useState<PreviewFilePersistedState | null>(null);
    const [dismissedPersistedCallId, setDismissedPersistedCallId] = useState<number | null>(null);

    const refreshPersistedState = useCallback(async () => {
        if (conversationId === undefined) {
            setPersistedState(null);
            return;
        }
        try {
            const calls = await invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", {
                conversationId,
            });
            const latest = getLatestPreviewFileState(calls, conversationId);
            if (!latest) {
                setPersistedState(null);
                return;
            }

            try {
                const normalizedRequest = await invoke<PreviewFileRequest>(
                    "prepare_preview_file_request_for_ui",
                    {
                        conversationId: latest.request.conversation_id ?? conversationId,
                        request: latest.request,
                    }
                );
                setPersistedState({
                    ...latest,
                    request: normalizedRequest,
                });
            } catch (error) {
                console.error(
                    "Failed to normalize persisted preview_file request:",
                    getErrorMessage(error)
                );
                setPersistedState(latest);
            }
        } catch (error) {
            console.error("Failed to load persisted preview_file state:", getErrorMessage(error));
        }
    }, [conversationId]);

    useEffect(() => {
        setPendingRequest(null);
        setIsDialogOpen(false);
        setDismissedPersistedCallId(null);
    }, [conversationId]);

    useEffect(() => {
        void refreshPersistedState();
    }, [refreshPersistedState]);

    useEffect(() => {
        const unsubscribe = listen<PreviewFileRequest>("preview-file-request", (event) => {
            const request = event.payload;

            if (
                conversationId !== undefined &&
                request.conversation_id !== undefined &&
                request.conversation_id !== null &&
                request.conversation_id !== conversationId
            ) {
                return;
            }

            setPendingRequest(request);
            setIsDialogOpen(true);
            void refreshPersistedState();
            setTimeout(() => {
                void refreshPersistedState();
            }, 300);
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId, refreshPersistedState]);

    useEffect(() => {
        if (conversationId === undefined) {
            return;
        }
        const eventName = `conversation_event_${conversationId}`;
        const unsubscribe = listen<any>(eventName, (event) => {
            const payload = event.payload as { type?: string; data?: { tool_name?: string } };
            if (payload?.type !== "mcp_tool_call_update") {
                return;
            }
            if (
                payload.data?.tool_name &&
                payload.data.tool_name !== PREVIEW_FILE_TOOL_NAME
            ) {
                return;
            }
            void refreshPersistedState();
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId, refreshPersistedState]);

    const previewFileView = useMemo(() => {
        if (pendingRequest && isDialogOpen) {
            let persistedAnchor: PreviewFilePersistedState | null = null;

            if (persistedState) {
                if (
                    persistedState.requestId &&
                    persistedState.requestId === pendingRequest.request_id
                ) {
                    persistedAnchor = persistedState;
                } else {
                    const pendingSignature = buildPreviewFileSignature(pendingRequest);
                    const persistedSignature = buildPreviewFileSignature(persistedState.request);
                    if (
                        pendingSignature &&
                        persistedSignature &&
                        pendingSignature === persistedSignature
                    ) {
                        persistedAnchor = persistedState;
                    } else if (
                        persistedState.status === "pending" ||
                        persistedState.status === "executing"
                    ) {
                        persistedAnchor = persistedState;
                    }
                }
            }

            return {
                request: pendingRequest,
                isOpen: true,
                callId: persistedAnchor?.callId ?? (null as number | null),
                messageId: persistedAnchor?.messageId ?? (null as number | null),
            };
        }

        if (
            persistedState &&
            persistedState.callId !== dismissedPersistedCallId &&
            (persistedState.status === "pending" ||
                persistedState.status === "executing" ||
                persistedState.status === "success")
        ) {
            return {
                request: persistedState.request,
                isOpen: true,
                callId: persistedState.callId,
                messageId: persistedState.messageId,
            };
        }

        return {
            request: null as PreviewFileRequest | null,
            isOpen: false,
            callId: null as number | null,
            messageId: null as number | null,
        };
    }, [pendingRequest, isDialogOpen, persistedState, dismissedPersistedCallId]);

    const handleOpenChange = useCallback((open: boolean) => {
        setIsDialogOpen(open);
        if (!open) {
            if (previewFileView.callId !== null) {
                setDismissedPersistedCallId(previewFileView.callId);
            }
            setPendingRequest(null);
        }
    }, [previewFileView.callId]);

    return {
        pendingRequest: previewFileView.request,
        isDialogOpen: previewFileView.isOpen,
        callId: previewFileView.callId,
        messageId: previewFileView.messageId,
        handleOpenChange,
    };
}
