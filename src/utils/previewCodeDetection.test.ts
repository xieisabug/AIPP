import { describe, expect, it } from "vitest";

import {
    messageContainsPreviewCode,
    messagesContainPreviewCode,
} from "@/utils/previewCodeDetection";

describe("previewCodeDetection", () => {
    it("detects preview_code in MCP json markup", () => {
        expect(
            messageContainsPreviewCode(
                '<!-- MCP_TOOL_CALL:{"tool_name":"preview_code","parameters":"{}"} -->',
            ),
        ).toBe(true);
    });

    it("detects preview_code in MCP xml markup", () => {
        expect(
            messageContainsPreviewCode(
                "<mcp_tool_call><tool_name>preview_code</tool_name></mcp_tool_call>",
            ),
        ).toBe(true);
    });

    it("ignores non-preview tool calls", () => {
        expect(
            messageContainsPreviewCode(
                '<!-- MCP_TOOL_CALL_STREAMING:{"tool_name":"demo_tool"} -->',
            ),
        ).toBe(false);
    });

    it("detects preview_code across message collections", () => {
        expect(
            messagesContainPreviewCode([
                { content: "plain text" },
                {
                    content:
                        '<!-- MCP_TOOL_CALL_STREAMING:{"tool_name":"preview_code"} -->',
                },
            ]),
        ).toBe(true);
    });
});
