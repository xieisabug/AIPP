import { useEffect, useRef } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { openConversationInChat } from "./openConversationInChat";

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
    /** 一并加进输入框的本机文件（如选中元素的截图）。 */
    attachments?: string[];
}

/** 发送修复请求；有聊天界面接收返回 true。 */
export async function requestArtifactFix(conversationId: number, text: string, attachments?: string[]): Promise<boolean> {
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
        await emit(REQUEST_EVENT, {
            request_id: requestId,
            conversation_id: conversationId,
            text,
            attachments: attachments?.length ? attachments : undefined,
        } satisfies FixRequestPayload);
        return await acknowledged;
    } finally {
        clearTimeout(timer);
        unlisten();
    }
}

/**
 * 发给不一定开着的会话（合集里的小工具）：没有界面接收时先在聊天窗口打开该会话，等它切换过去后重试。
 * `open` 与 `wait` 可替换，便于测试。
 */
export async function requestArtifactFixOpening(
    conversationId: number,
    text: string,
    attachments?: string[],
    deps: {
        request?: typeof requestArtifactFix;
        open?: (conversationId: number) => Promise<void>;
        wait?: (ms: number) => Promise<void>;
    } = {},
): Promise<boolean> {
    const request = deps.request ?? requestArtifactFix;
    const open = deps.open ?? openConversationInChat;
    const wait = deps.wait ?? ((ms: number) => new Promise<void>((resolve) => setTimeout(resolve, ms)));
    if (await request(conversationId, text, attachments)) return true;
    await open(conversationId);
    for (let attempt = 0; attempt < 4; attempt += 1) {
        await wait(500);
        if (await request(conversationId, text, attachments)) return true;
    }
    return false;
}

/** 聊天界面接收发给当前会话的修复请求。 */
export function useArtifactFixRequests(
    conversationId: number | null,
    onRequest: (text: string, attachments: string[]) => void,
) {
    const onRequestRef = useRef(onRequest);
    onRequestRef.current = onRequest;

    useEffect(() => {
        if (conversationId === null) return;
        const subscription = listen<FixRequestPayload>(REQUEST_EVENT, ({ payload }) => {
            if (payload.conversation_id !== conversationId) return;
            onRequestRef.current(payload.text, payload.attachments ?? []);
            void emit(ACK_EVENT, { request_id: payload.request_id });
        });
        return () => {
            void subscription.then((unlisten) => unlisten());
        };
    }, [conversationId]);
}
