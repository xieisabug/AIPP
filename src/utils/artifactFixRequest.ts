import { useEffect, useRef } from "react";
import { emit, listen } from "@tauri-apps/api/event";

/**
 * 预览窗口把 Artifact 报错交给原会话：广播请求，正在显示该会话的聊天界面把报告填进输入框
 * （不自动发送，用户可补充说明）并回执。没有界面回执说明该会话没打开。
 */
const REQUEST_EVENT = "artifact-fix-request";
const ACK_EVENT = "artifact-fix-request-ack";
const ACK_TIMEOUT_MS = 1500;

interface FixRequestPayload {
    request_id: string;
    conversation_id: number;
    text: string;
}

/** 发送修复请求；有聊天界面接收返回 true。 */
export async function requestArtifactFix(conversationId: number, text: string): Promise<boolean> {
    const requestId = `${Date.now()}-${Math.random().toString(36).slice(2)}`;
    let resolveAck: (handled: boolean) => void = () => {};
    const acknowledged = new Promise<boolean>((resolve) => {
        resolveAck = resolve;
    });
    const unlisten = await listen<{ request_id: string }>(ACK_EVENT, ({ payload }) => {
        if (payload.request_id === requestId) resolveAck(true);
    });
    const timer = setTimeout(() => resolveAck(false), ACK_TIMEOUT_MS);
    try {
        await emit(REQUEST_EVENT, { request_id: requestId, conversation_id: conversationId, text } satisfies FixRequestPayload);
        return await acknowledged;
    } finally {
        clearTimeout(timer);
        unlisten();
    }
}

/** 聊天界面接收发给当前会话的修复请求。 */
export function useArtifactFixRequests(conversationId: number | null, onRequest: (text: string) => void) {
    const onRequestRef = useRef(onRequest);
    onRequestRef.current = onRequest;

    useEffect(() => {
        if (conversationId === null) return;
        const subscription = listen<FixRequestPayload>(REQUEST_EVENT, ({ payload }) => {
            if (payload.conversation_id !== conversationId) return;
            onRequestRef.current(payload.text);
            void emit(ACK_EVENT, { request_id: payload.request_id });
        });
        return () => {
            void subscription.then((unlisten) => unlisten());
        };
    }, [conversationId]);
}
