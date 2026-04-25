import { Dispatch, SetStateAction, useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
    OperationPermissionRequest,
    AcpPermissionRequest,
} from "@/components/OperationPermissionDialog";
import { getErrorMessage } from "@/utils/error";

interface UseOperationPermissionOptions {
    /** 当前会话 ID，用于过滤只处理当前会话的权限请求 */
    conversationId?: number;
    /** 当前窗口可处理的一组会话 ID */
    conversationIds?: number[];
}

interface PermissionRequestBase {
    request_id: string;
    conversation_id?: number;
}

interface PermissionResolvedEvent {
    request_id: string;
    conversation_id?: number;
}

function isStaleAcpPermissionError(message: string) {
    return (
        message.includes("ACP permission request not found or already resolved") ||
        message.includes("ACP permission receiver dropped before resolution")
    );
}

function isStaleOperationPermissionError(message: string) {
    return (
        message.includes("Permission request not found or already resolved") ||
        message.includes("Permission request receiver dropped before resolution")
    );
}

function shouldHandleConversationRequest(
    requestConversationId: number | undefined,
    conversationId?: number,
    conversationIds?: number[]
) {
    if (requestConversationId === undefined) {
        return true;
    }
    if (conversationIds && conversationIds.length > 0) {
        return conversationIds.includes(requestConversationId);
    }
    if (conversationId !== undefined) {
        return requestConversationId === conversationId;
    }
    return true;
}

interface UsePermissionRequestQueueOptions {
    conversationId?: number;
    conversationIds?: number[];
    requestEventName: string;
    resolvedEventName: string;
    logLabel: string;
}

interface PermissionRequestQueueController<TRequest extends PermissionRequestBase> {
    pendingRequest: TRequest | null;
    isDialogOpen: boolean;
    decisionError: string | null;
    isSubmitting: boolean;
    removeRequestById: (requestId: string) => void;
    setDecisionError: Dispatch<SetStateAction<string | null>>;
    setIsSubmitting: Dispatch<SetStateAction<boolean>>;
}

function usePermissionRequestQueue<TRequest extends PermissionRequestBase>(
    options: UsePermissionRequestQueueOptions
): PermissionRequestQueueController<TRequest> {
    const {
        conversationId,
        conversationIds,
        requestEventName,
        resolvedEventName,
        logLabel,
    } = options;
    const [requestQueue, setRequestQueue] = useState<TRequest[]>([]);
    const [decisionError, setDecisionError] = useState<string | null>(null);
    const [isSubmitting, setIsSubmitting] = useState(false);
    const pendingRequest = requestQueue[0] ?? null;
    const isDialogOpen = pendingRequest !== null;

    const removeRequestById = useCallback((requestId: string) => {
        setRequestQueue((prev) => {
            const removedIndex = prev.findIndex((item) => item.request_id === requestId);
            if (removedIndex === -1) {
                return prev;
            }
            if (removedIndex === 0) {
                setDecisionError(null);
                setIsSubmitting(false);
            }
            return prev.filter((item) => item.request_id !== requestId);
        });
    }, []);

    useEffect(() => {
        const unsubscribe = listen<TRequest>(requestEventName, (event) => {
            const request = event.payload;
            if (
                !shouldHandleConversationRequest(
                    request.conversation_id,
                    conversationId,
                    conversationIds
                )
            ) {
                return;
            }

            console.log(`Received ${logLabel} request:`, request);
            setRequestQueue((prev) => {
                if (prev.some((item) => item.request_id === request.request_id)) {
                    return prev;
                }
                if (prev.length === 0) {
                    setDecisionError(null);
                    setIsSubmitting(false);
                }
                return [...prev, request];
            });
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId, conversationIds, logLabel, requestEventName]);

    useEffect(() => {
        const unsubscribe = listen<PermissionResolvedEvent>(resolvedEventName, (event) => {
            const requestId = event.payload?.request_id;
            if (!requestId) {
                return;
            }
            console.log(`Received ${logLabel} resolution:`, event.payload);
            removeRequestById(requestId);
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [logLabel, removeRequestById, resolvedEventName]);

    return {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        removeRequestById,
        setDecisionError,
        setIsSubmitting,
    };
}

export function useOperationPermission(options: UseOperationPermissionOptions = {}) {
    const { conversationId, conversationIds } = options;
    const {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        removeRequestById,
        setDecisionError,
        setIsSubmitting,
    } = usePermissionRequestQueue<OperationPermissionRequest>({
        conversationId,
        conversationIds,
        requestEventName: "operation-permission-request",
        resolvedEventName: "operation-permission-resolved",
        logLabel: "operation permission",
    });

    const handleDecision = useCallback(
        async (
            requestId: string,
            decision:
                | "allow"
                | "allow_for_conversation"
                | "allow_for_assistant"
                | "allow_and_save"
                | "deny"
        ) => {
            if (
                !pendingRequest ||
                pendingRequest.request_id !== requestId ||
                isSubmitting
            ) {
                return;
            }
            setIsSubmitting(true);
            try {
                console.log("Sending permission decision:", { requestId, decision });
                await invoke("confirm_operation_permission", {
                    requestId,
                    decision,
                });
                setDecisionError(null);
                removeRequestById(requestId);
            } catch (error) {
                const message = getErrorMessage(error) || "提交权限决策失败";
                console.error("Failed to send permission decision:", message);
                if (isStaleOperationPermissionError(message)) {
                    removeRequestById(requestId);
                    setDecisionError(null);
                    return;
                }
                setDecisionError(message);
                setIsSubmitting(false);
            }
        },
        [
            isSubmitting,
            pendingRequest,
            removeRequestById,
            setDecisionError,
            setIsSubmitting,
        ]
    );

    return {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        handleDecision,
    };
}

interface UseAcpPermissionOptions {
    conversationId?: number;
    conversationIds?: number[];
}

export function useAcpPermission(options: UseAcpPermissionOptions = {}) {
    const { conversationId, conversationIds } = options;
    const {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        removeRequestById,
        setDecisionError,
        setIsSubmitting,
    } = usePermissionRequestQueue<AcpPermissionRequest>({
        conversationId,
        conversationIds,
        requestEventName: "acp-permission-request",
        resolvedEventName: "acp-permission-resolved",
        logLabel: "ACP permission",
    });

    const handleDecision = useCallback(
        async (requestId: string, optionId?: string, cancelled?: boolean) => {
            if (
                !pendingRequest ||
                pendingRequest.request_id !== requestId ||
                isSubmitting
            ) {
                return;
            }
            setIsSubmitting(true);
            try {
                console.log("Sending ACP permission decision:", { requestId, optionId, cancelled });
                await invoke("confirm_acp_permission", {
                    requestId,
                    optionId: optionId ?? null,
                    cancelled: cancelled ?? false,
                });
                setDecisionError(null);
                removeRequestById(requestId);
            } catch (error) {
                const message = getErrorMessage(error) || "提交 ACP 权限决策失败";
                console.error("Failed to send ACP permission decision:", message);
                if (isStaleAcpPermissionError(message)) {
                    removeRequestById(requestId);
                    setDecisionError(null);
                    return;
                }
                setDecisionError(message);
                setIsSubmitting(false);
            }
        },
        [
            isSubmitting,
            pendingRequest,
            removeRequestById,
            setDecisionError,
            setIsSubmitting,
        ]
    );

    return {
        pendingRequest,
        isDialogOpen,
        decisionError,
        isSubmitting,
        handleDecision,
    };
}
