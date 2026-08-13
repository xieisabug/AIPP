import { describe, expect, it } from "vitest";

import {
    LARGE_MESSAGE_PREVIEW_HEIGHT_ESTIMATE,
    getLargeMessagePreviewStats,
    shouldUseLargeMessagePreview,
} from "./largeMessagePreview";

describe("largeMessagePreview", () => {
    it("does not preview plain large historical responses in phase 1", () => {
        const content = Array.from(
            { length: 260 },
            (_, index) => `plain response line ${index}`,
        ).join("\n");

        expect(
            shouldUseLargeMessagePreview({
                content,
                isLastMessage: false,
                isStreaming: false,
                messageType: "response",
            }),
        ).toBe(false);
    });

    it("defers historical tool results regardless of size", () => {
        const content = Array.from(
            { length: 2 },
            (_, index) => `tool result line ${index}`,
        ).join("\n");
        const stats = getLargeMessagePreviewStats(content, "tool_result");

        expect(stats.shouldPreview).toBe(true);
        expect(stats.reason).toBe("tool_result");
        expect(stats.previewText).toBe("");
        expect(LARGE_MESSAGE_PREVIEW_HEIGHT_ESTIMATE).toBeGreaterThan(0);
    });

    it("defers response messages with MCP payloads regardless of size", () => {
        const payload = "x".repeat(12);
        const content = `<!-- MCP_TOOL_CALL:${JSON.stringify({
            call_id: 1751,
            tool_name: "write_file",
            parameters: payload,
        })} -->\nvisible tail`;
        const stats = getLargeMessagePreviewStats(content, "response");

        expect(stats.shouldPreview).toBe(true);
        expect(stats.reason).toBe("mcp_payload");
        expect(stats.summary).toContain("write_file");
        expect(stats.previewText).toBe("");
    });
});
