import { Message } from "@/data/Conversation";

type MessageOrderingFields = Pick<
    Message,
    "id" | "message_type" | "generation_group_id" | "created_time" | "start_time" | "finish_time"
>;

function tryParseOrderValue(
    value: MessageOrderingFields["created_time"] | MessageOrderingFields["start_time"] | MessageOrderingFields["finish_time"],
): number | null {
    const timestamp = value ? new Date(value as Date | string).getTime() : NaN;
    return Number.isFinite(timestamp) ? timestamp : null;
}

function compareReasoningResponseWhenTied(
    a: MessageOrderingFields,
    b: MessageOrderingFields,
): number {
    if (!a.generation_group_id || a.generation_group_id !== b.generation_group_id) {
        return 0;
    }

    const aIsReasoning = a.message_type === "reasoning";
    const bIsReasoning = b.message_type === "reasoning";
    const aIsResponse = a.message_type === "response";
    const bIsResponse = b.message_type === "response";

    if ((aIsReasoning && bIsResponse) || (aIsResponse && bIsReasoning)) {
        return aIsReasoning ? -1 : 1;
    }

    return 0;
}

export function getMessageOrderValue(message: MessageOrderingFields): number {
    return (
        tryParseOrderValue(message.created_time) ??
        tryParseOrderValue(message.start_time) ??
        tryParseOrderValue(message.finish_time) ??
        message.id
    );
}

export function compareMessagesChronologically(
    a: MessageOrderingFields,
    b: MessageOrderingFields,
): number {
    const orderDelta = getMessageOrderValue(a) - getMessageOrderValue(b);
    if (orderDelta !== 0) {
        return orderDelta;
    }

    const reasoningTieBreak = compareReasoningResponseWhenTied(a, b);
    if (reasoningTieBreak !== 0) {
        return reasoningTieBreak;
    }

    return a.id - b.id;
}
