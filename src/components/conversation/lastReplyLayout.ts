import type { Message } from "@/data/Conversation";

export interface LastReplyEntryLike {
    messageId: number;
}

export function findLastReplyStartIndex(
    allDisplayMessages: Pick<Message, "id" | "message_type">[],
    messageElements: LastReplyEntryLike[],
): number {
    for (let i = allDisplayMessages.length - 1; i >= 0; i -= 1) {
        if (allDisplayMessages[i].message_type !== "user") {
            continue;
        }

        return messageElements.findIndex(
            (entry) => entry.messageId === allDisplayMessages[i].id,
        );
    }

    return -1;
}
