import { useMemo, useRef } from "react";
import type { AgentActivityEvent, MCPToolCallUpdateEvent, Message } from "@/data/Conversation";
import { useLinger } from "@/hooks/useStablePresence";
import type { InlineInteractionItem } from "../ConversationUI";
import type { InteractionRow } from "./butlerInteractions";
import { hasPublishedReply, withButlerProcessing, withPeerTimeline, type PeerPost } from "./butlerTimeline";
import { reportedProgressByMessage } from "./reportedProgress";

const BUTLER_IDLE_GAP_MS = 1200;

interface TranscriptOptions {
    conversationId: number;
    compact: boolean;
    running: boolean;
    messages: Message[];
    posts?: PeerPost[];
    interactions?: InteractionRow[];
    liveItems?: InlineInteractionItem[];
    calls: ReadonlyMap<number, MCPToolCallUpdateEvent>;
    activities: ReadonlyMap<string, AgentActivityEvent>;
}

export function useButlerTranscript({ conversationId, compact, running, messages, posts, interactions, liveItems, calls, activities }: TranscriptOptions) {
    const held = useLinger(compact && running, BUTLER_IDLE_GAP_MS, conversationId);
    const turnIds = useMemo(() => {
        const ids = new Set<number>();
        if (!compact) return ids;
        // 用原始消息划分轮次，简洁时间线可能隐藏了本轮的触发消息。
        for (let index = messages.length - 1; index >= 0; index--) {
            const message = messages[index];
            ids.add(message.id);
            if (message.message_type === "user" && (message.id > 0 || message.sendStatus === "sending")) break;
        }
        return ids;
    }, [compact, messages]);

    // 发送动作早于用户消息入库，保留已经发布的回复，避免上一轮在发送时闪烁。
    const settledTail = useRef<{ conversationId: number; ids: Set<number> } | null>(null);
    if (compact && !running && !held) {
        settledTail.current = { conversationId, ids: turnIds };
    }

    return useMemo(() => {
        if (!compact) return withButlerProcessing(messages, conversationId, running);
        const unpublished = new Set<number>();
        if (running || held) {
            const settled = settledTail.current?.conversationId === conversationId ? settledTail.current.ids : undefined;
            for (const id of turnIds) {
                if (!settled?.has(id)) unpublished.add(id);
            }
        }
        const reports = reportedProgressByMessage(conversationId, activities.values(), calls.values(), interactions);
        const visible = withPeerTimeline(messages, unpublished, posts ?? [], conversationId, interactions, liveItems, calls, reports);
        // 插话会新增用户消息，运行中的工具仍属于原回复。
        const publishedReports = visible.filter(row => row.message_type === "butler_report").map(row => ({
            text: row.content, anchor: row.parent_id ?? null, request_id: row.butler_report_request_id,
            currentTurn: turnIds.has(row.parent_id!),
        }));
        return withButlerProcessing(visible, conversationId, running || (held && !hasPublishedReply(visible)),
            publishedReports);
    }, [compact, running, held, conversationId, messages, turnIds, posts, interactions, liveItems, calls, activities]);
}
