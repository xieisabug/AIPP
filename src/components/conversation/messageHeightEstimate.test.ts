import { describe, expect, it } from "vitest";

import {
    estimateMarkdownHeight,
    estimateMergedGroupHeight,
    estimateMessageHeight,
} from "./messageHeightEstimate";

const toolCall = (toolName: string, callId = 1) =>
    `<!-- MCP_TOOL_CALL:${JSON.stringify({
        server_name: "Agent 工具",
        tool_name: toolName,
        parameters: { path: "a".repeat(2000) },
        call_id: callId,
    })} -->`;

function message(message_type: string, content: string, finish_time: Date | null = new Date()) {
    return { message_type, content, finish_time } as Parameters<typeof estimateMessageHeight>[0];
}

describe("estimateMessageHeight", () => {
    it("should return 0 when a finished reasoning message is hidden by the display config", () => {
        expect(
            estimateMessageHeight(message("reasoning", "long thinking\n".repeat(200)), {
                mergedMode: true,
                isShowThinking: false,
            }),
        ).toBe(0);
    });

    it("should keep a compact row when a finished reasoning message is shown collapsed", () => {
        const height = estimateMessageHeight(message("reasoning", "long thinking\n".repeat(200)), {
            mergedMode: true,
            isShowThinking: true,
        });
        expect(height).toBeGreaterThan(30);
        expect(height).toBeLessThan(100);
    });

    it("should size a tool-only response as a single tool card regardless of payload size", () => {
        const height = estimateMessageHeight(message("response", `\n\n${toolCall("call_mcp_tool")}\n`), {
            mergedMode: true,
        });
        expect(height).toBeGreaterThanOrEqual(40);
        expect(height).toBeLessThan(90);
    });

    it("should reserve the preview viewport when a response contains preview_code", () => {
        const height = estimateMessageHeight(message("response", toolCall("preview_code")), {
            mergedMode: true,
        });
        expect(height).toBeGreaterThan(300);
        expect(height).toBeLessThan(400);
    });

    it("should estimate a one-line user message as one bubble line", () => {
        expect(estimateMessageHeight(message("user", "继续"))).toBe(54);
    });

    it("should estimate less wrapping when the list is wider", () => {
        const content = "这是一段比较长的中文说明文字，用来验证换行估算。".repeat(20);
        const narrow = estimateMessageHeight(message("response", content), { mergedMode: true, listWidthPx: 600 });
        const wide = estimateMessageHeight(message("response", content), { mergedMode: true, listWidthPx: 1400 });
        expect(wide).toBeLessThan(narrow);
    });
});

describe("estimateMarkdownHeight", () => {
    it("should cap auto-collapsed code blocks at the collapsed line count", () => {
        const longCode = ["```ts", ...Array.from({ length: 300 }, (_, i) => `const v${i} = ${i};`), "```"].join("\n");
        const shortCode = ["```ts", ...Array.from({ length: 16 }, (_, i) => `const v${i} = ${i};`), "```"].join("\n");
        expect(estimateMarkdownHeight(longCode, 600)).toBe(estimateMarkdownHeight(shortCode, 600));
    });

    it("should not count table separator rows", () => {
        const table = "| a | b |\n|---|---|\n| 1 | 2 |";
        const tableWithoutSeparator = "| a | b |\n| 1 | 2 |";
        expect(estimateMarkdownHeight(table, 600)).toBe(estimateMarkdownHeight(tableWithoutSeparator, 600));
    });
});

describe("estimateMergedGroupHeight", () => {
    it("should skip parts that render nothing when adding gaps", () => {
        expect(estimateMergedGroupHeight([0, 100, 0, 50])).toBe(
            estimateMergedGroupHeight([100, 50]),
        );
        expect(estimateMergedGroupHeight([100, 50])).toBeGreaterThan(150);
    });
});
