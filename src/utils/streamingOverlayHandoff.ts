import type { StreamEvent } from "@/data/Conversation";

/**
 * When SSE completes, a short final throttle frame may still sit in the rAF queue.
 * Prefer the longer body so live UI / handoff cannot clip the canonical tail.
 */
export function finalizeStreamingEventWithPending(
    doneEvent: StreamEvent,
    pending: StreamEvent | undefined,
): StreamEvent {
    if (!pending) {
        return { ...doneEvent, is_done: true };
    }

    const content =
        pending.message_id === doneEvent.message_id
            && pending.message_type === doneEvent.message_type
            && pending.content.length > doneEvent.content.length
            && pending.content.startsWith(doneEvent.content)
            ? pending.content
            : doneEvent.content;

    return {
        ...doneEvent,
        content,
        is_done: true,
    };
}

/**
 * Resolve bubble text during streaming and overlay→persisted handoff.
 * After is_done, keep the longer of stream overlay vs messages[] so a stale
 * messages snapshot cannot drop trailing characters that the overlay already has.
 */
export function resolveMessageDisplayContent(
    messageContent: string,
    streamEvent?: Pick<StreamEvent, "content" | "is_done"> | null,
): string {
    if (!streamEvent) {
        return messageContent;
    }

    if (!streamEvent.is_done) {
        return streamEvent.content;
    }

    return streamEvent.content.length >= messageContent.length
        ? streamEvent.content
        : messageContent;
}
