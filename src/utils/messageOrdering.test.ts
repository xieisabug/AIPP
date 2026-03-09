import { describe, expect, it } from "vitest";
import {
    compareMessagesChronologically,
    getMessageOrderValue,
} from "@/utils/messageOrdering";
import { Message } from "@/data/Conversation";

function makeMessage(
    id: number,
    messageType: Message["message_type"],
    createdTime: string,
    generationGroupId: string | null = null,
    overrides: Partial<Message> = {},
): Message {
    return {
        id,
        conversation_id: 1,
        message_type: messageType,
        content: `${messageType}-${id}`,
        llm_model_id: null,
        created_time: new Date(createdTime),
        start_time: null,
        finish_time: null,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: generationGroupId,
        parent_group_id: null,
        parent_id: null,
        regenerate: null,
        ...overrides,
    };
}

describe("compareMessagesChronologically", () => {
    it("keeps same-generation messages in real time order", () => {
        const messages = [
            makeMessage(2, "response", "2024-01-01T00:00:01.000Z", "g1"),
            makeMessage(3, "reasoning", "2024-01-01T00:00:02.000Z", "g1"),
        ];

        const sorted = [...messages].sort(compareMessagesChronologically);
        expect(sorted.map((message) => message.id)).toEqual([2, 3]);
    });

    it("uses reasoning before response only as a same-generation tie-break", () => {
        const messages = [
            makeMessage(2, "response", "2024-01-01T00:00:01.000Z", "g1"),
            makeMessage(1, "reasoning", "2024-01-01T00:00:01.000Z", "g1"),
        ];

        const sorted = [...messages].sort(compareMessagesChronologically);
        expect(sorted.map((message) => message.id)).toEqual([1, 2]);
    });

    it("does not cluster different generations by reasoning/response when timestamps tie", () => {
        const messages = [
            makeMessage(4, "response", "2024-01-01T00:00:01.000Z", "g3"),
            makeMessage(2, "response", "2024-01-01T00:00:01.000Z", "g1"),
            makeMessage(5, "reasoning", "2024-01-01T00:00:01.000Z", "g4"),
            makeMessage(3, "reasoning", "2024-01-01T00:00:01.000Z", "g2"),
        ];

        const sorted = [...messages].sort(compareMessagesChronologically);
        expect(sorted.map((message) => message.id)).toEqual([2, 3, 4, 5]);
    });

    it("reorders only the tied generation instead of clustering all reasoning first", () => {
        const messages = [
            makeMessage(2, "response", "2024-01-01T00:00:01.000Z", "g1"),
            makeMessage(3, "response", "2024-01-01T00:00:02.000Z", "g2"),
            makeMessage(4, "reasoning", "2024-01-01T00:00:02.000Z", "g2"),
            makeMessage(5, "response", "2024-01-01T00:00:02.000Z", "g3"),
            makeMessage(6, "reasoning", "2024-01-01T00:00:02.000Z", "g4"),
        ];

        const sorted = [...messages].sort(compareMessagesChronologically);
        expect(sorted.map((message) => message.id)).toEqual([2, 4, 3, 5, 6]);
    });
});

describe("getMessageOrderValue", () => {
    it("falls back to start_time when created_time is invalid", () => {
        const message = makeMessage(
            7,
            "response",
            "invalid-date",
            "g1",
            {
                start_time: new Date("2024-01-01T00:00:03.000Z"),
            },
        );

        expect(getMessageOrderValue(message)).toBe(
            new Date("2024-01-01T00:00:03.000Z").getTime(),
        );
    });

    it("falls back to finish_time when created_time and start_time are invalid", () => {
        const message = makeMessage(
            8,
            "response",
            "invalid-date",
            "g1",
            {
                start_time: new Date("invalid-date"),
                finish_time: new Date("2024-01-01T00:00:04.000Z"),
            },
        );

        expect(getMessageOrderValue(message)).toBe(
            new Date("2024-01-01T00:00:04.000Z").getTime(),
        );
    });

    it("falls back to id when all timestamps are invalid", () => {
        const message = makeMessage(
            9,
            "response",
            "invalid-date",
            "g1",
            {
                start_time: new Date("invalid-date"),
                finish_time: new Date("invalid-date"),
            },
        );

        expect(getMessageOrderValue(message)).toBe(9);
    });
});
