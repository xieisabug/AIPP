import { Message, StreamEvent } from "@/data/Conversation";
import { compareMessagesChronologically } from "@/utils/messageOrdering";

export interface MergeMessagesWithStreamingStateOptions {
    conversationId: number;
    currentMessages?: Message[];
    streamingSnapshot?: ReadonlyMap<number, StreamEvent>;
    finalizeStreaming?: boolean;
    finalizedAt?: Date;
}

const STREAMING_MESSAGE_TYPES = new Set<Message["message_type"]>([
    "assistant",
    "response",
    "reasoning",
    "error",
]);

function shouldPreferLocalContent(baseMessage: Message, localMessage: Message): boolean {
    if (!STREAMING_MESSAGE_TYPES.has(localMessage.message_type)) {
        return false;
    }

    if (!localMessage.content) {
        return false;
    }

    if (!baseMessage.content) {
        return true;
    }

    if (localMessage.message_type !== baseMessage.message_type) {
        return true;
    }

    return localMessage.content.length > baseMessage.content.length;
}

function mergeBaseAndLocalMessage(baseMessage: Message, localMessage: Message): Message {
    const preferLocalContent = shouldPreferLocalContent(baseMessage, localMessage);

    return {
        ...baseMessage,
        message_type: preferLocalContent
            ? localMessage.message_type
            : baseMessage.message_type,
        content: preferLocalContent ? localMessage.content : baseMessage.content,
        llm_model_id: baseMessage.llm_model_id ?? localMessage.llm_model_id,
        created_time: baseMessage.created_time ?? localMessage.created_time,
        start_time: baseMessage.start_time ?? localMessage.start_time ?? null,
        finish_time: baseMessage.finish_time ?? localMessage.finish_time ?? null,
        token_count: Math.max(baseMessage.token_count ?? 0, localMessage.token_count ?? 0),
        input_token_count: Math.max(
            baseMessage.input_token_count ?? 0,
            localMessage.input_token_count ?? 0
        ),
        output_token_count: Math.max(
            baseMessage.output_token_count ?? 0,
            localMessage.output_token_count ?? 0
        ),
        generation_group_id:
            baseMessage.generation_group_id ?? localMessage.generation_group_id ?? null,
        parent_group_id:
            baseMessage.parent_group_id ?? localMessage.parent_group_id ?? null,
        parent_id: baseMessage.parent_id ?? localMessage.parent_id ?? null,
        regenerate: baseMessage.regenerate ?? localMessage.regenerate ?? null,
        attachment_list: baseMessage.attachment_list ?? localMessage.attachment_list,
        tool_calls_json: baseMessage.tool_calls_json ?? localMessage.tool_calls_json ?? null,
        first_token_time:
            baseMessage.first_token_time ?? localMessage.first_token_time ?? null,
        ttft_ms: baseMessage.ttft_ms ?? localMessage.ttft_ms ?? null,
        tps: baseMessage.tps ?? localMessage.tps ?? null,
    };
}

function buildMessageFromStreamEvent(
    existingMessages: Message[],
    streamEvent: StreamEvent,
    conversationId: number,
    finishTime: Date | null
): Message {
    const lastUserMessage = [...existingMessages]
        .reverse()
        .find((message) => message.message_type === "user");
    const baseTime = lastUserMessage ? new Date(lastUserMessage.created_time) : new Date();
    const offsetMs = streamEvent.message_type === "reasoning" ? 500 : 1000;

    return {
        id: streamEvent.message_id,
        conversation_id: conversationId,
        message_type: streamEvent.message_type,
        content: streamEvent.content,
        llm_model_id: null,
        created_time: new Date(baseTime.getTime() + offsetMs),
        start_time: streamEvent.message_type === "reasoning" ? baseTime : null,
        finish_time: finishTime,
        token_count: streamEvent.token_count ?? 0,
        input_token_count: streamEvent.input_token_count ?? 0,
        output_token_count: streamEvent.output_token_count ?? 0,
        generation_group_id: null,
        parent_group_id: null,
        parent_id: null,
        regenerate: null,
        ttft_ms: streamEvent.ttft_ms ?? null,
        tps: streamEvent.tps ?? null,
    };
}

function applyStreamEventToMessage(
    message: Message | undefined,
    existingMessages: Message[],
    streamEvent: StreamEvent,
    options: MergeMessagesWithStreamingStateOptions
): Message {
    const finalizedAt = options.finalizedAt ?? new Date();
    const finishTime =
        options.finalizeStreaming || streamEvent.is_done
            ? streamEvent.end_time ?? finalizedAt
            : message?.finish_time ?? null;

    if (!message) {
        return buildMessageFromStreamEvent(
            existingMessages,
            streamEvent,
            options.conversationId,
            finishTime
        );
    }

    return {
        ...message,
        message_type: streamEvent.message_type,
        content: streamEvent.content,
        finish_time: finishTime,
        token_count: streamEvent.token_count ?? message.token_count,
        input_token_count: streamEvent.input_token_count ?? message.input_token_count,
        output_token_count: streamEvent.output_token_count ?? message.output_token_count,
        ttft_ms: streamEvent.ttft_ms ?? message.ttft_ms,
        tps: streamEvent.tps ?? message.tps,
    };
}

export function mergeMessagesWithStreamingState(
    baseMessages: Message[],
    options: MergeMessagesWithStreamingStateOptions
): Message[] {
    const {
        currentMessages = [],
        streamingSnapshot = new Map<number, StreamEvent>(),
    } = options;
    const mergedById = new Map<number, Message>();

    baseMessages.forEach((message) => {
        mergedById.set(message.id, { ...message });
    });

    currentMessages.forEach((message) => {
        const existingMessage = mergedById.get(message.id);
        if (!existingMessage) {
            mergedById.set(message.id, { ...message });
            return;
        }

        mergedById.set(message.id, mergeBaseAndLocalMessage(existingMessage, message));
    });

    streamingSnapshot.forEach((streamEvent) => {
        const existingMessages = Array.from(mergedById.values());
        const existingMessage = mergedById.get(streamEvent.message_id);
        mergedById.set(
            streamEvent.message_id,
            applyStreamEventToMessage(existingMessage, existingMessages, streamEvent, options)
        );
    });

    return Array.from(mergedById.values()).sort(compareMessagesChronologically);
}
