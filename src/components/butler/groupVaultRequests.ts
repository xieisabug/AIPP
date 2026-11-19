import type { ButlerGroupMessage } from "@/data/ButlerGroup";

export interface GroupVaultRequest {
    requestId: string;
    conversationId: number | null;
    messageId: number | null;
    groupAnchor?: { groupId: number; seq: number } | null;
}

/** Never approximate a source ID with the next/latest message: that moves cards between turns. */
export function groupVaultRequests<T extends GroupVaultRequest>(groupId: number, messages: ButlerGroupMessage[], requests: T[]) {
    const bySeq = new Map<number, T[]>();
    const pending: T[] = [];
    const seen = new Set<string>();
    for (const request of requests) {
        if (seen.has(request.requestId)) continue;
        seen.add(request.requestId);
        const owner = request.groupAnchor
            ? messages.find(message => request.groupAnchor!.groupId === groupId && message.group_id === groupId
                && message.seq === request.groupAnchor!.seq && message.kind === "message")
            : messages.find(message => message.group_id === groupId && message.kind === "message"
                && request.conversationId !== null && request.messageId !== null
                && message.source_conversation_id === request.conversationId && message.source_message_id === request.messageId);
        if (!owner) { pending.push(request); continue; }
        bySeq.set(owner.seq, [...(bySeq.get(owner.seq) ?? []), request]);
    }
    return { bySeq, pending };
}
