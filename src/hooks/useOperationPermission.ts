import { useCallback, useEffect, useState } from "react";
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
}

export function useOperationPermission(options: UseOperationPermissionOptions = {}) {
    const { conversationId } = options;
    const [pendingRequest, setPendingRequest] = useState<OperationPermissionRequest | null>(null);
    const [, setRequestQueue] = useState<OperationPermissionRequest[]>([]);
    const [isDialogOpen, setIsDialogOpen] = useState(false);
    const [decisionError, setDecisionError] = useState<string | null>(null);

    const shiftNextRequest = useCallback(() => {
        setRequestQueue((prev) => {
            const [, ...rest] = prev;
            const next = rest[0] ?? null;
            setPendingRequest(next);
            setIsDialogOpen(rest.length > 0);
            if (rest.length > 0) {
                setDecisionError(null);
            }
            return rest;
        });
    }, []);

    useEffect(() => {
        const unsubscribe = listen<OperationPermissionRequest>(
            "operation-permission-request",
            (event) => {
                const request = event.payload;

                // 如果指定了 conversationId，只处理匹配的请求
                // 如果请求没有 conversation_id，则显示给所有窗口
                if (
                    conversationId !== undefined &&
                    request.conversation_id !== undefined &&
                    request.conversation_id !== conversationId
                ) {
                    return;
                }

                console.log("Received operation permission request:", request);
                setRequestQueue((prev) => {
                    const next = [...prev, request];
                    if (next.length === 1) {
                        setPendingRequest(request);
                        setIsDialogOpen(true);
                        setDecisionError(null);
                    }
                    return next;
                });
            }
        );

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId]);

    const handleDecision = useCallback(
        async (requestId: string, decision: "allow" | "allow_and_save" | "deny") => {
            if (!pendingRequest || pendingRequest.request_id !== requestId) {
                return;
            }
            try {
                console.log("Sending permission decision:", { requestId, decision });
                await invoke("confirm_operation_permission", {
                    requestId,
                    decision,
                });
                setDecisionError(null);
                shiftNextRequest();
            } catch (error) {
                const message = getErrorMessage(error) || "提交权限决策失败";
                console.error("Failed to send permission decision:", message);
                setDecisionError(message);
            }
        },
        [pendingRequest, shiftNextRequest]
    );

    return {
        pendingRequest,
        isDialogOpen,
        decisionError,
        handleDecision,
    };
}

interface UseAcpPermissionOptions {
    conversationId?: number;
}

export function useAcpPermission(options: UseAcpPermissionOptions = {}) {
    const { conversationId } = options;
    const [pendingRequest, setPendingRequest] = useState<AcpPermissionRequest | null>(null);
    const [, setRequestQueue] = useState<AcpPermissionRequest[]>([]);
    const [isDialogOpen, setIsDialogOpen] = useState(false);
    const [decisionError, setDecisionError] = useState<string | null>(null);

    const shiftNextRequest = useCallback(() => {
        setRequestQueue((prev) => {
            const [, ...rest] = prev;
            const next = rest[0] ?? null;
            setPendingRequest(next);
            setIsDialogOpen(rest.length > 0);
            if (rest.length > 0) {
                setDecisionError(null);
            }
            return rest;
        });
    }, []);

    useEffect(() => {
        const unsubscribe = listen<AcpPermissionRequest>("acp-permission-request", (event) => {
            const request = event.payload;

            if (
                conversationId !== undefined &&
                request.conversation_id !== undefined &&
                request.conversation_id !== conversationId
            ) {
                return;
            }

            console.log("Received ACP permission request:", request);
            setRequestQueue((prev) => {
                const next = [...prev, request];
                if (next.length === 1) {
                    setPendingRequest(request);
                    setIsDialogOpen(true);
                    setDecisionError(null);
                }
                return next;
            });
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId]);

    const handleDecision = useCallback(
        async (requestId: string, optionId?: string, cancelled?: boolean) => {
            if (!pendingRequest || pendingRequest.request_id !== requestId) {
                return;
            }
            try {
                console.log("Sending ACP permission decision:", { requestId, optionId, cancelled });
                await invoke("confirm_acp_permission", {
                    requestId,
                    optionId: optionId ?? null,
                    cancelled: cancelled ?? false,
                });
                setDecisionError(null);
                shiftNextRequest();
            } catch (error) {
                const message = getErrorMessage(error) || "提交 ACP 权限决策失败";
                console.error("Failed to send ACP permission decision:", message);
                setDecisionError(message);
            }
        },
        [pendingRequest, shiftNextRequest]
    );

    return {
        pendingRequest,
        isDialogOpen,
        decisionError,
        handleDecision,
    };
}
