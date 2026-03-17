import { describe, expect, it } from "vitest";

import { Message, StreamEvent } from "@/data/Conversation";
import { mergeMessagesWithStreamingState } from "@/utils/streamingMessageState";

function makeMessage(
    id: number,
    messageType: Message["message_type"],
    content: string,
    createdTime: string,
    overrides: Partial<Message> = {}
): Message {
    return {
        id,
        conversation_id: 1,
        message_type: messageType,
        content,
        llm_model_id: null,
        created_time: new Date(createdTime),
        start_time: null,
        finish_time: null,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: null,
        parent_group_id: null,
        parent_id: null,
        regenerate: null,
        ...overrides,
    };
}

function makeStreamEvent(
    messageId: number,
    messageType: StreamEvent["message_type"],
    content: string,
    overrides: Partial<StreamEvent> = {}
): StreamEvent {
    return {
        message_id: messageId,
        message_type: messageType,
        content,
        is_done: false,
        ...overrides,
    };
}

describe("mergeMessagesWithStreamingState", () => {
    it("materializes the latest streamed content when stopping", () => {
        const messages = [
            makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z"),
            makeMessage(2, "response", "最开始", "2024-01-01T00:00:01.000Z"),
        ];
        const finalizedAt = new Date("2024-01-01T00:00:05.000Z");

        const merged = mergeMessagesWithStreamingState(messages, {
            conversationId: 1,
            currentMessages: messages,
            streamingSnapshot: new Map([
                [
                    2,
                    makeStreamEvent(2, "response", "最终停下来的内容", {
                        token_count: 12,
                        output_token_count: 12,
                    }),
                ],
            ]),
            finalizeStreaming: true,
            finalizedAt,
        });

        expect(merged).toHaveLength(2);
        expect(merged[1].content).toBe("最终停下来的内容");
        expect(merged[1].finish_time).toEqual(finalizedAt);
        expect(merged[1].token_count).toBe(12);
        expect(merged[1].output_token_count).toBe(12);
    });

    it("preserves richer local content when a late message reload returns stale data", () => {
        const serverMessages = [
            makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z"),
            makeMessage(2, "response", "最开始", "2024-01-01T00:00:01.000Z", {
                generation_group_id: "group-1",
            }),
        ];
        const currentMessages = [
            makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z"),
            makeMessage(2, "response", "已经显示到更后面的完整内容", "2024-01-01T00:00:01.000Z"),
        ];

        const merged = mergeMessagesWithStreamingState(serverMessages, {
            conversationId: 1,
            currentMessages,
        });

        expect(merged).toHaveLength(2);
        expect(merged[1].content).toBe("已经显示到更后面的完整内容");
        expect(merged[1].generation_group_id).toBe("group-1");
    });

    it("creates a streamed message when stop happens before the base message is reloaded", () => {
        const messages = [makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z")];
        const finalizedAt = new Date("2024-01-01T00:00:05.000Z");

        const merged = mergeMessagesWithStreamingState(messages, {
            conversationId: 1,
            currentMessages: messages,
            streamingSnapshot: new Map([
                [2, makeStreamEvent(2, "response", "新消息也要保留下来")],
            ]),
            finalizeStreaming: true,
            finalizedAt,
        });

        expect(merged).toHaveLength(2);
        expect(merged[1].id).toBe(2);
        expect(merged[1].content).toBe("新消息也要保留下来");
        expect(merged[1].finish_time).toEqual(finalizedAt);
    });
});
