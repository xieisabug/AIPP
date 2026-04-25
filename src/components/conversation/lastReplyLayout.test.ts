import { describe, expect, it } from "vitest";
import { findLastReplyStartIndex } from "./lastReplyLayout";

describe("findLastReplyStartIndex", () => {
    it("uses the last user message element index in non-merged mode", () => {
        const allDisplayMessages = [
            { id: 1, message_type: "user" as const },
            { id: 2, message_type: "response" as const },
            { id: 3, message_type: "user" as const },
            { id: 4, message_type: "response" as const },
        ];
        const messageElements = [
            { messageId: 1 },
            { messageId: 2 },
            { messageId: 3 },
            { messageId: 4 },
        ];

        expect(
            findLastReplyStartIndex(allDisplayMessages, messageElements),
        ).toBe(2);
    });

    it("finds the last user message even when assistant messages are merged", () => {
        const allDisplayMessages = [
            { id: 1, message_type: "user" as const },
            { id: 2, message_type: "reasoning" as const },
            { id: 3, message_type: "response" as const },
            { id: 4, message_type: "user" as const },
            { id: 5, message_type: "reasoning" as const },
            { id: 6, message_type: "tool_result" as const },
            { id: 7, message_type: "response" as const },
        ];
        const messageElements = [
            { messageId: 1 },
            { messageId: 3 },
            { messageId: 4 },
            { messageId: 7 },
        ];

        expect(
            findLastReplyStartIndex(allDisplayMessages, messageElements),
        ).toBe(2);
    });

    it("returns -1 when no user message exists", () => {
        const allDisplayMessages = [
            { id: 2, message_type: "reasoning" as const },
            { id: 3, message_type: "response" as const },
        ];
        const messageElements = [{ messageId: 3 }];

        expect(
            findLastReplyStartIndex(allDisplayMessages, messageElements),
        ).toBe(-1);
    });
});
