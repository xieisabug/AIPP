import { describe, expect, it } from "vitest";
import type { StreamEvent } from "@/data/Conversation";
import {
    finalizeStreamingEventWithPending,
    resolveMessageDisplayContent,
} from "./streamingOverlayHandoff";

function streamEvent(partial: Partial<StreamEvent> & Pick<StreamEvent, "content" | "is_done">): StreamEvent {
    return {
        message_id: 1,
        message_type: "response",
        ...partial,
    };
}

describe("finalizeStreamingEventWithPending", () => {
    it("keeps done content when there is no pending throttle frame", () => {
        const done = streamEvent({ content: "Hello world!", is_done: true });
        expect(finalizeStreamingEventWithPending(done, undefined).content).toBe("Hello world!");
        expect(finalizeStreamingEventWithPending(done, undefined).is_done).toBe(true);
    });

    it("consumes a longer short-final pending frame instead of dropping it on complete", () => {
        const done = streamEvent({ content: "Hello wor", is_done: true });
        const pending = streamEvent({ content: "Hello world!", is_done: false });
        const finalized = finalizeStreamingEventWithPending(done, pending);
        expect(finalized.content).toBe("Hello world!");
        expect(finalized.is_done).toBe(true);
    });

    it("prefers done content when it is at least as long as pending", () => {
        const done = streamEvent({ content: "Hello world!", is_done: true });
        const pending = streamEvent({ content: "Hello wor", is_done: false });
        expect(finalizeStreamingEventWithPending(done, pending).content).toBe("Hello world!");
    });

    it("should keep the final replacement when an older frame has unrelated longer content", () => {
        const done = streamEvent({ content: "corrected", is_done: true });
        const pending = streamEvent({ content: "older unrelated output is longer", is_done: false });
        expect(finalizeStreamingEventWithPending(done, pending).content).toBe("corrected");
    });

    it("should keep an error when the pending frame contains a longer response", () => {
        const done = streamEvent({ content: "error", is_done: true, message_type: "error" });
        const pending = streamEvent({ content: "error-free earlier response", is_done: false });
        expect(finalizeStreamingEventWithPending(done, pending).content).toBe("error");
    });
});

describe("resolveMessageDisplayContent", () => {
    it("uses stream overlay while streaming", () => {
        expect(
            resolveMessageDisplayContent("stale", {
                content: "live tail",
                is_done: false,
            }),
        ).toBe("live tail");
    });

    it("keeps longer overlay content during is_done handoff to persisted message", () => {
        expect(
            resolveMessageDisplayContent("Hello wor", {
                content: "Hello world!",
                is_done: true,
            }),
        ).toBe("Hello world!");
    });

    it("falls back to persisted message when overlay is absent or shorter", () => {
        expect(resolveMessageDisplayContent("canonical full", null)).toBe("canonical full");
        expect(
            resolveMessageDisplayContent("canonical full text", {
                content: "short",
                is_done: true,
            }),
        ).toBe("canonical full text");
    });
});
