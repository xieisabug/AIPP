import { describe, expect, it } from "vitest";

import { Message, StreamEvent } from "@/data/Conversation";
import { mergeMessagesWithStreamingState, reuseEquivalentMessages } from "@/utils/streamingMessageState";

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

describe("reuseEquivalentMessages", () => {
    // IPC returns a fresh object graph: ISO strings, new arrays, nulls for absent fields.
    const fromIpc = (message: Message): Message => JSON.parse(JSON.stringify({
        ...message,
        attachment_list: message.attachment_list ?? [],
        tps: null,
    }));

    it("should keep previous message objects when a refetch returns identical data", () => {
        const previous = [
            makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z", { attachment_list: [] }),
            makeMessage(2, "response", "回答", "2024-01-01T00:00:01.000Z", { finish_time: new Date("2024-01-01T00:00:02.000Z") }),
        ];

        const result = reuseEquivalentMessages(previous, previous.map(fromIpc));

        expect(result).toBe(previous);
    });

    it("should replace only the changed message when a refetch adds or edits messages", () => {
        const previous = [
            makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z"),
            makeMessage(2, "response", "回答", "2024-01-01T00:00:01.000Z"),
        ];
        const next = [
            fromIpc(previous[0]),
            fromIpc({ ...previous[1], content: "回答（已编辑）" }),
            fromIpc(makeMessage(3, "user", "继续", "2024-01-01T00:00:03.000Z")),
        ];

        const result = reuseEquivalentMessages(previous, next);

        expect(result[0]).toBe(previous[0]);
        expect(result[1]).toBe(next[1]);
        expect(result[2]).toBe(next[2]);
    });

    it("should keep untouched history identities when merging a message_add refetch", () => {
        const current = [
            makeMessage(1, "user", "你好", "2024-01-01T00:00:00.000Z"),
            makeMessage(2, "response", "回答", "2024-01-01T00:00:01.000Z"),
        ];
        const refetched = [
            ...current.map(fromIpc),
            fromIpc(makeMessage(3, "user", "新问题", "2024-01-01T00:00:05.000Z")),
        ];

        const merged = mergeMessagesWithStreamingState(refetched, {
            conversationId: 1,
            currentMessages: current,
        });

        expect(merged.map((message) => message.id)).toEqual([1, 2, 3]);
        expect(merged[0]).toBe(current[0]);
        expect(merged[1]).toBe(current[1]);
        expect(merged[2].content).toBe("新问题");
    });
});
