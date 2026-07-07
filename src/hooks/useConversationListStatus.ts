import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import type {
    ConversationListActivityEvent,
    ConversationListItemStatus,
} from "@/data/Conversation";

function setsEqual(a: Set<number>, b: Set<number>): boolean {
    if (a.size !== b.size) {
        return false;
    }
    for (const value of a) {
        if (!b.has(value)) {
            return false;
        }
    }
    return true;
}

function updateRunningIds(
    prev: Set<number>,
    conversationId: number,
    isRunning: boolean,
): Set<number> {
    const next = new Set(prev);
    if (isRunning) {
        next.add(conversationId);
    } else {
        next.delete(conversationId);
    }
    return setsEqual(prev, next) ? prev : next;
}

function updateUnreadIds(
    prev: Set<number>,
    conversationId: number,
    shouldMarkUnread: boolean,
): Set<number> {
    const next = new Set(prev);
    if (shouldMarkUnread) {
        next.add(conversationId);
    } else {
        next.delete(conversationId);
    }
    return setsEqual(prev, next) ? prev : next;
}

export function useConversationListStatus(activeConversationId: string) {
    const [runningIds, setRunningIds] = useState<Set<number>>(() => new Set());
    const [unreadIds, setUnreadIds] = useState<Set<number>>(() => new Set());
    const activeConversationIdRef = useRef(activeConversationId);

    useEffect(() => {
        activeConversationIdRef.current = activeConversationId;
    }, [activeConversationId]);

    useEffect(() => {
        const activeId = Number(activeConversationId);
        if (!Number.isFinite(activeId) || activeId <= 0) {
            return;
        }

        setUnreadIds((prev) => updateUnreadIds(prev, activeId, false));
    }, [activeConversationId]);

    useEffect(() => {
        let unlistenListActivity: (() => void) | undefined;
        let unlistenDeleted: (() => void) | undefined;
        let disposed = false;

        const setupListeners = async () => {
            try {
                const runningConversationIds = await invoke<number[]>(
                    "list_running_conversation_ids",
                );
                if (!disposed) {
                    setRunningIds(new Set(runningConversationIds));
                }
            } catch (error) {
                console.error(
                    "useConversationListStatus: failed to sync running conversations",
                    error,
                );
            }

            unlistenListActivity = await listen<ConversationListActivityEvent>(
                "conversation_list_activity",
                (event) => {
                    const payload = event.payload;
                    if (!payload?.conversation_id) {
                        return;
                    }

                    const conversationId = payload.conversation_id;

                    if (payload.kind === "runtime_state") {
                        const isRunning = payload.is_running === true;
                        setRunningIds((prev) =>
                            updateRunningIds(prev, conversationId, isRunning),
                        );
                        if (isRunning) {
                            setUnreadIds((prev) =>
                                updateUnreadIds(prev, conversationId, false),
                            );
                        }
                        return;
                    }

                    if (payload.kind === "stream_complete") {
                        const activeId = Number(activeConversationIdRef.current);
                        const shouldMarkUnread =
                            !Number.isFinite(activeId) ||
                            activeId <= 0 ||
                            conversationId !== activeId;
                        if (shouldMarkUnread) {
                            setUnreadIds((prev) =>
                                updateUnreadIds(prev, conversationId, true),
                            );
                        }
                    }
                },
            );

            unlistenDeleted = await listen<number>("conversation_deleted", (event) => {
                const deletedConversationId = event.payload;
                if (!deletedConversationId) {
                    return;
                }

                setRunningIds((prev) => updateRunningIds(prev, deletedConversationId, false));
                setUnreadIds((prev) => updateUnreadIds(prev, deletedConversationId, false));
            });
        };

        void setupListeners();

        return () => {
            disposed = true;
            unlistenListActivity?.();
            unlistenDeleted?.();
        };
    }, []);

    const getItemStatus = useCallback(
        (conversationId: number): ConversationListItemStatus => {
            if (runningIds.has(conversationId)) {
                return "responding";
            }
            if (unreadIds.has(conversationId)) {
                return "completed_unread";
            }
            return "idle";
        },
        [runningIds, unreadIds],
    );

    return { getItemStatus };
}