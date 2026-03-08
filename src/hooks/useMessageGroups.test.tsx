import { renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { Message } from "@/data/Conversation";
import { useMessageGroups } from "@/hooks/useMessageGroups";

function makeMessage(
    id: number,
    messageType: Message["message_type"],
    createdTime: string,
    generationGroupId: string,
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

describe("useMessageGroups ordering", () => {
    it("sorts messages inside a version with only a local reasoning-response tie-break", () => {
        const messages = [
            makeMessage(2, "response", "2024-01-01T00:00:01.000Z", "g1"),
            makeMessage(3, "response", "2024-01-01T00:00:02.000Z", "g2"),
            makeMessage(4, "reasoning", "2024-01-01T00:00:02.000Z", "g2"),
            makeMessage(5, "response", "2024-01-01T00:00:02.000Z", "g3"),
            makeMessage(6, "reasoning", "2024-01-01T00:00:02.000Z", "g4"),
        ];

        const { result } = renderHook(() =>
            useMessageGroups({
                allDisplayMessages: messages,
                groupMergeMap: new Map(),
            }),
        );

        expect(
            result.current.generationGroups
                .get("g2")
                ?.versions[0]
                .messages.map((message) => message.id),
        ).toEqual([4, 3]);
        expect(
            result.current.generationGroups
                .get("g3")
                ?.versions[0]
                .messages.map((message) => message.id),
        ).toEqual([5]);
        expect(
            result.current.generationGroups
                .get("g4")
                ?.versions[0]
                .messages.map((message) => message.id),
        ).toEqual([6]);
    });
});
