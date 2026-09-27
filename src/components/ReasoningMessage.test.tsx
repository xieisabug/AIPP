import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Message } from "../data/Conversation";
import ReasoningMessage from "./ReasoningMessage";

const parseCustomTags = vi.hoisted(() => vi.fn((content: string) => content));

vi.mock("react-markdown", () => ({
    default: ({ children }: { children: React.ReactNode }) => (
        <div data-testid="reasoning-markdown">{children}</div>
    ),
}));

vi.mock("../hooks/useCustomTagParser", () => ({
    useCustomTagParser: () => ({ parseCustomTags }),
}));

vi.mock("../hooks/useMarkdownConfig", () => ({
    useMarkdownConfig: () => ({
        remarkPlugins: [],
        rehypePlugins: [],
        markdownComponents: {},
    }),
}));

vi.mock("../hooks/useMcpToolCallProcessor", () => ({
    useMcpToolCallProcessor: () => ({
        processContent: (_content: string, element: React.ReactNode) => element,
    }),
}));

function createReasoningMessage(): Message {
    return {
        id: 1,
        conversation_id: 1,
        message_type: "reasoning",
        content: "",
        llm_model_id: null,
        created_time: new Date("2026-09-27T00:00:00Z"),
        start_time: new Date("2026-09-27T00:00:00Z"),
        finish_time: new Date("2026-09-27T00:00:01Z"),
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        regenerate: null,
    };
}

describe("ReasoningMessage rendering cost", () => {
    beforeEach(() => {
        parseCustomTags.mockReset().mockImplementation((content: string) => content);
    });

    it("skips full content parsing for completed collapsed reasoning", () => {
        const content = `${"large reasoning line\\n".repeat(5000)}tail`;

        render(
            <ReasoningMessage
                message={createReasoningMessage()}
                displayedContent={content}
                isReasoningExpanded={false}
            />,
        );

        expect(screen.getByText(/^思考完成/)).toBeInTheDocument();
        expect(screen.queryByTestId("reasoning-markdown")).not.toBeInTheDocument();
        expect(parseCustomTags).not.toHaveBeenCalled();
    });

    it("parses content when completed reasoning is expanded", () => {
        render(
            <ReasoningMessage
                message={createReasoningMessage()}
                displayedContent="expanded reasoning"
                isReasoningExpanded
            />,
        );

        expect(screen.getByTestId("reasoning-markdown")).toBeInTheDocument();
        expect(parseCustomTags).toHaveBeenCalledWith("expanded reasoning");
    });

    it.each([
        '<!-- MCP_TOOL_CALL:{"tool_name":"example"} -->',
        '<mcp_tool_call><tool_name>example</tool_name></mcp_tool_call>',
    ])("should keep the tool preview when collapsed reasoning contains %s", (content) => {
        const parsed = '<!-- MCP_TOOL_CALL:{"tool_name":"example"} -->';
        parseCustomTags.mockReturnValue(parsed);
        render(
            <ReasoningMessage
                message={createReasoningMessage()}
                displayedContent={content}
                isReasoningExpanded={false}
            />,
        );
        expect(parseCustomTags).toHaveBeenCalledWith(content);
        expect(screen.getByTestId("reasoning-markdown")).toHaveTextContent(parsed);
    });
});
