import { describe, expect, it } from "vitest";
import type { Message } from "@/data/Conversation";
import {
    collectPreferredRetryCopyContent,
    groupMessagesByStreamRetryAttempt,
    parseStreamRetryMeta,
    shouldShowStreamRetryGrouping,
} from "./streamRetryAttempts";

function makeMessage(
    id: number,
    content: string,
    overrides: Partial<Message> = {},
): Message {
    return {
        id,
        conversation_id: 1,
        message_type: "response",
        content,
        llm_model_id: null,
        created_time: new Date("2026-11-01T00:00:00Z"),
        start_time: new Date("2026-11-01T00:00:00Z"),
        finish_time: new Date("2026-11-01T00:00:01Z"),
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        generation_group_id: null,
        parent_group_id: null,
        regenerate: null,
        ...overrides,
    };
}

function retryMeta(
    attempt: number,
    status: string,
    sessionId = "sess-1",
    error?: string,
): string {
    return JSON.stringify({
        stream_retry: {
            session_id: sessionId,
            attempt,
            status,
            ...(error ? { error } : {}),
        },
    });
}

describe("parseStreamRetryMeta", () => {
    it("reads stream_retry metadata", () => {
        const message = makeMessage(1, "partial", {
            metadata_json: retryMeta(1, "interrupted", "abc", "drop"),
        });
        expect(parseStreamRetryMeta(message)).toEqual({
            sessionId: "abc",
            attempt: 1,
            status: "interrupted",
            error: "drop",
        });
    });

    it("returns null for ordinary messages", () => {
        expect(parseStreamRetryMeta(makeMessage(1, "hi"))).toBeNull();
    });
});

describe("groupMessagesByStreamRetryAttempt", () => {
    it("groups consecutive messages by attempt and keeps flat order", () => {
        const messages = [
            makeMessage(1, "think1", {
                message_type: "reasoning",
                metadata_json: retryMeta(1, "interrupted"),
            }),
            makeMessage(2, "partial-1", {
                metadata_json: retryMeta(1, "interrupted", "sess-1", "boom"),
            }),
            makeMessage(3, "think2", {
                message_type: "reasoning",
                metadata_json: retryMeta(2, "completed"),
            }),
            makeMessage(4, "final", {
                metadata_json: retryMeta(2, "completed"),
            }),
        ];
        const buckets = groupMessagesByStreamRetryAttempt(messages);
        expect(buckets).toHaveLength(2);
        expect(buckets[0]).toMatchObject({
            attempt: 1,
            status: "interrupted",
            error: "boom",
        });
        expect(buckets[0].messages.map((m) => m.id)).toEqual([1, 2]);
        expect(buckets[1]).toMatchObject({
            attempt: 2,
            status: "completed",
        });
        expect(buckets[1].messages.map((m) => m.id)).toEqual([3, 4]);
        expect(shouldShowStreamRetryGrouping(buckets)).toBe(true);
    });

    it("does not enable grouping for a single successful attempt without interrupt", () => {
        const buckets = groupMessagesByStreamRetryAttempt([
            makeMessage(1, "only"),
        ]);
        expect(shouldShowStreamRetryGrouping(buckets)).toBe(false);
    });

    it("collapses old interrupted partials while keeping preferred copy on latest", () => {
        const messages = [
            makeMessage(1, "old partial", {
                metadata_json: retryMeta(1, "interrupted"),
            }),
            makeMessage(2, "newer final", {
                metadata_json: retryMeta(2, "completed"),
            }),
        ];
        expect(collectPreferredRetryCopyContent(messages)).toBe("newer final");
    });
});
