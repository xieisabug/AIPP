import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
    OperationPermissionRequest,
    AcpPermissionRequest,
} from "@/components/OperationPermissionDialog";

interface UseOperationPermissionOptions {
    /** 当前会话 ID，用于过滤只处理当前会话的权限请求 */
    conversationId?: number;
}

export function useOperationPermission(options: UseOperationPermissionOptions = {}) {
    const { conversationId } = options;
    const [pendingRequest, setPendingRequest] = useState<OperationPermissionRequest | null>(null);
    const [isDialogOpen, setIsDialogOpen] = useState(false);

    useEffect(() => {
        const unsubscribe = listen<OperationPermissionRequest>(
            "operation-permission-request",
            (event) => {
                const request = event.payload;
                
                // 如果指定了 conversationId，只处理匹配的请求
                // 如果请求没有 conversation_id，则显示给所有窗口
                if (conversationId !== undefined && 
                    request.conversation_id !== undefined && 
                    request.conversation_id !== conversationId) {
                    return;
                }

                console.log("Received operation permission request:", request);
                setPendingRequest(request);
                setIsDialogOpen(true);
            }
        );

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId]);

    const handleDecision = useCallback(
        async (requestId: string, decision: 'allow' | 'allow_and_save' | 'deny') => {
            try {
                console.log("Sending permission decision:", { requestId, decision });
                await invoke("confirm_operation_permission", {
                    requestId,
                    decision,
                });
                setIsDialogOpen(false);
                setPendingRequest(null);
            } catch (error) {
                console.error("Failed to send permission decision:", error);
                // 即使失败也关闭对话框，避免卡住
                setIsDialogOpen(false);
                setPendingRequest(null);
            }
        },
        []
    );

    return {
        pendingRequest,
        isDialogOpen,
        handleDecision,
    };
}

interface UseAcpPermissionOptions {
    conversationId?: number;
}

export function useAcpPermission(options: UseAcpPermissionOptions = {}) {
    const { conversationId } = options;
    const [pendingRequest, setPendingRequest] = useState<AcpPermissionRequest | null>(null);
    const [isDialogOpen, setIsDialogOpen] = useState(false);

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
            setPendingRequest(request);
            setIsDialogOpen(true);
        });

        return () => {
            unsubscribe.then((f) => f());
        };
    }, [conversationId]);

    const handleDecision = useCallback(
        async (requestId: string, optionId?: string, cancelled?: boolean) => {
            try {
                console.log("Sending ACP permission decision:", { requestId, optionId, cancelled });
                await invoke("confirm_acp_permission", {
                    requestId,
                    optionId: optionId ?? null,
                    cancelled: cancelled ?? false,
                });
                setIsDialogOpen(false);
                setPendingRequest(null);
            } catch (error) {
                console.error("Failed to send ACP permission decision:", error);
                setIsDialogOpen(false);
                setPendingRequest(null);
            }
        },
        []
    );

    return {
        pendingRequest,
        isDialogOpen,
        handleDecision,
    };
}
