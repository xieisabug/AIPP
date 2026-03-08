import { describe, expect, it } from "vitest";
import { compareMessagesChronologically } from "@/utils/messageOrdering";
import { Message } from "@/data/Conversation";

function makeMessage(
    id: number,
    messageType: Message["message_type"],
    createdTime: string,
    generationGroupId: string | null = null,
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
});
