import { describe, expect, it } from "vitest";
import type { ConversationSearchHit } from "../data/Conversation";
import { formatHitTime, speakerLabel } from "./conversationSearchFormat";

const hit = (overrides: Partial<ConversationSearchHit>): ConversationSearchHit => ({
    conversation_id: 1, conversation_name: "c", assistant_name: "a", message_id: 1, message_type: null,
    created_time: new Date(), snippet: "", hit_type: "message", ...overrides,
});

describe("conversationSearchFormat", () => {
    it("should prefer explicit speaker over message type label", () => {
        expect(speakerLabel(hit({ speaker: "审稿", message_type: "user" }))).toBe("审稿");
        expect(speakerLabel(hit({ message_type: "response" }))).toBe("助手");
        expect(speakerLabel(hit({ message_type: "custom" }))).toBe("custom");
        expect(speakerLabel(hit({}))).toBe("");
    });

    it("should shorten time by distance from now", () => {
        const now = new Date(2026, 10, 7, 18, 0);
        expect(formatHitTime(new Date(2026, 10, 7, 9, 5), now)).toBe("09:05");
        expect(formatHitTime(new Date(2026, 2, 1, 20, 30), now)).toBe("3月1日 20:30");
        expect(formatHitTime(new Date(2024, 0, 2, 8, 0), now)).toBe("2024/1/2");
        expect(formatHitTime("not a date", now)).toBe("");
    });
});
