import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, UnlistenFn } from "@tauri-apps/api/event";
import {
    ConversationEvent,
    ActivityFocus,
    ActivityFocusChangeEvent,
} from "../data/Conversation";

export interface UseActivityFocusOptions {
    conversationId: string | number | undefined;
}

export interface ActivityFocusState {
    /** 当前活动焦点 */
    focus: ActivityFocus;
    /** 当前需要显示闪亮边框的消息 ID（如果有） */
    shiningMessageId: number | null;
    /** 当前需要显示闪亮边框的 MCP 调用 ID（如果有） */
    shiningMcpCallId: number | null;
    /** 检查指定消息是否应该显示闪亮边框 */
    isMessageShining: (messageId: number) => boolean;
    /** 检查指定 MCP 调用是否应该显示闪亮边框 */
    isMcpCallShining: (callId: number) => boolean;
    /** 手动清除活动焦点（用于取消等场景） */
    clearFocus: () => void;
}

/**
 * 活动焦点管理 Hook
 * 
 * 监听后端发送的 activity_focus_change 事件，统一管理闪亮边框状态。
 * 这是新的设计，后端作为状态的唯一来源，前端只负责消费和展示。
 */
export function useActivityFocus(options: UseActivityFocusOptions): ActivityFocusState {
    const [focus, setFocus] = useState<ActivityFocus>({ focus_type: 'none' });
    const unsubscribeRef = useRef<UnlistenFn | null>(null);
    const focusSyncRequestIdRef = useRef<number>(0);

    // 从 focus 派生出闪亮状态
    const shiningMessageId: number | null =
        focus.focus_type === 'user_pending' || focus.focus_type === 'assistant_streaming'
            ? focus.message_id
            : null;

    const shiningMcpCallId: number | null =
        focus.focus_type === 'mcp_executing'
            ? focus.call_id
            : null;

    // 检查消息是否应该显示闪亮边框
    const isMessageShining = useCallback((messageId: number): boolean => {
        if (focus.focus_type === 'user_pending' || focus.focus_type === 'assistant_streaming') {
            return focus.message_id === messageId;
        }
        return false;
    }, [focus]);

    // 检查 MCP 调用是否应该显示闪亮边框
    const isMcpCallShining = useCallback((callId: number): boolean => {
        if (focus.focus_type === 'mcp_executing') {
            return focus.call_id === callId;
        }
        return false;
    }, [focus]);

    // 手动清除活动焦点
    const clearFocus = useCallback(() => {
        setFocus({ focus_type: 'none' });
    }, []);

    // 处理事件
    const handleEvent = useCallback((event: any) => {
        const conversationEvent = event.payload as ConversationEvent;

        if (conversationEvent.type === 'activity_focus_change') {
            const focusEvent = conversationEvent.data as ActivityFocusChangeEvent;
            console.log('[ActivityFocus] Received focus change:', focusEvent.focus);
            setFocus(focusEvent.focus);
        }

        // 兼容处理：stream_complete 和 conversation_cancel 事件也应该清除焦点
        // 这是为了与旧事件保持兼容，在后端完全切换到新机制后可以移除
        if (conversationEvent.type === 'stream_complete' || conversationEvent.type === 'conversation_cancel') {
            console.log('[ActivityFocus] Clearing focus due to', conversationEvent.type);
            setFocus({ focus_type: 'none' });
        }
    }, []);

    // 设置事件监听
    useEffect(() => {
        const { conversationId } = options;

        if (!conversationId) {
            focusSyncRequestIdRef.current += 1; // 避免旧请求落到新对话
            setFocus({ focus_type: 'none' });
            return;
        }

        const conversationIdNum = Number(conversationId);
        if (Number.isNaN(conversationIdNum)) {
            focusSyncRequestIdRef.current += 1;
            setFocus({ focus_type: 'none' });
            return;
        }

        const eventName = `conversation_event_${conversationIdNum}`;
        console.log('[ActivityFocus] Setting up listener for:', eventName);

        // 清理之前的监听
        if (unsubscribeRef.current) {
            unsubscribeRef.current();
            unsubscribeRef.current = null;
        }

        // 设置新的监听
        listen(eventName, handleEvent).then((unsubscribe) => {
            unsubscribeRef.current = unsubscribe;
        });

        // 同步当前的焦点状态，避免在订阅前的事件导致状态缺失
        const requestId = focusSyncRequestIdRef.current + 1;
        focusSyncRequestIdRef.current = requestId;
        invoke<ActivityFocus>("get_activity_focus", { conversationId: conversationIdNum })
            .then((currentFocus) => {
                if (focusSyncRequestIdRef.current !== requestId) return;
                console.log("[ActivityFocus] Synced focus from backend:", currentFocus);
                setFocus(currentFocus);
            })
            .catch((error) => {
                if (focusSyncRequestIdRef.current !== requestId) return;
                console.warn("[ActivityFocus] Failed to sync focus state", error);
            });

        return () => {
            if (unsubscribeRef.current) {
                unsubscribeRef.current();
                unsubscribeRef.current = null;
            }
        };
    }, [options.conversationId, handleEvent]);

    return {
        focus,
        shiningMessageId,
        shiningMcpCallId,
        isMessageShining,
        isMcpCallShining,
        clearFocus,
    };
}
