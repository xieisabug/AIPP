import React from "react";
import { render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { useMcpToolCallProcessor } from "@/hooks/useMcpToolCallProcessor";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/contexts/AntiLeakageContext", () => ({
    useAntiLeakage: () => ({
        enabled: false,
        isRevealed: true,
    }),
}));

vi.mock("@/components/magicui/shine-border", () => ({
    ShineBorder: () => <div data-testid="shine-border" />,
}));

interface HarnessProps {
    markdown: string;
    conversationId?: number;
    messageId?: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    shiningMcpCallId?: number | null;
}

const ProcessorHarness: React.FC<HarnessProps> = ({
    markdown,
    conversationId = 1,
    messageId = 1,
    mcpToolCallStates,
    shiningMcpCallId = null,
}) => {
    const { processContent } = useMcpToolCallProcessor(
        {
            remarkPlugins: [],
            rehypePlugins: [],
            markdownComponents: {},
        },
        {
            conversationId,
            messageId,
            mcpToolCallStates,
            shiningMcpCallId,
        },
    );

    return processContent(markdown, <div data-testid="fallback">fallback</div>);
};

describe("useMcpToolCallProcessor MCP identity", () => {
    const originalResizeObserver = globalThis.ResizeObserver;

    beforeAll(() => {
        class MockResizeObserver implements ResizeObserver {
            observe(): void { }
            unobserve(): void { }
            disconnect(): void { }
        }

        globalThis.ResizeObserver = MockResizeObserver;
    });

    afterAll(() => {
        globalThis.ResizeObserver = originalResizeObserver;
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("keeps identical tool parameters separated by call_id", async () => {
        const conversationId = 20;
        const firstCallId = 101;
        const secondCallId = 102;

        mockInvokeHandler("get_mcp_tool_call", (args) => ({
            id: Number(args?.callId),
            conversation_id: conversationId,
            message_id: 10,
            server_id: 1,
            server_name: "demo-server",
            tool_name: "demo-tool",
            parameters: '{"query":"hello"}',
            status: Number(args?.callId) === secondCallId ? "executing" : "pending",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [firstCallId, {
                call_id: firstCallId,
                conversation_id: conversationId,
                status: "pending",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: '{"query":"hello"}',
            }],
            [secondCallId, {
                call_id: secondCallId,
                conversation_id: conversationId,
                status: "executing",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: '{"query":"hello"}',
            }],
        ]);

        const markdown = [
            '<!-- MCP_TOOL_CALL: {"call_id":101,"server_name":"demo-server","tool_name":"demo-tool","parameters":"{\\"query\\":\\"hello\\"}"} -->',
            '<!-- MCP_TOOL_CALL: {"call_id":102,"server_name":"demo-server","tool_name":"demo-tool","parameters":"{\\"query\\":\\"hello\\"}"} -->',
        ].join("\n");

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={10}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={secondCallId}
            />,
        );

        expect(await screen.findByText("待执行")).toBeInTheDocument();
        expect(await screen.findByText("执行中")).toBeInTheDocument();
        expect(screen.getAllByText("demo-tool")).toHaveLength(2);
        expect(screen.getAllByTestId("shine-border")).toHaveLength(1);
    });

    it("upgrades a placeholder card when the streamed call_id arrives", async () => {
        const conversationId = 21;
        const resolvedCallId = 205;

        mockInvokeHandler("get_mcp_tool_call", (args) => ({
            id: Number(args?.callId),
            conversation_id: conversationId,
            message_id: 11,
            server_id: 1,
            server_name: "demo-server",
            tool_name: "demo-tool",
            parameters: '{"query":"hello"}',
            status: "executing",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const { rerender } = render(
            <ProcessorHarness
                markdown='<!-- MCP_TOOL_CALL: {"server_name":"demo-server","tool_name":"demo-tool","parameters":"{\"query\":\"hello\"}"} -->'
                conversationId={conversationId}
                messageId={11}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />,
        );

        expect(screen.queryByText("执行中")).not.toBeInTheDocument();
        expect(screen.queryByTestId("shine-border")).not.toBeInTheDocument();
        expect(screen.getAllByText("demo-tool")).toHaveLength(1);

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [resolvedCallId, {
                call_id: resolvedCallId,
                conversation_id: conversationId,
                status: "executing",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: '{"query":"hello"}',
            }],
        ]);

        rerender(
            <ProcessorHarness
                markdown='<!-- MCP_TOOL_CALL: {"call_id":205,"server_name":"demo-server","tool_name":"demo-tool","parameters":"{\"query\":\"hello\"}"} -->'
                conversationId={conversationId}
                messageId={11}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={resolvedCallId}
            />,
        );

        expect(await screen.findByText("执行中")).toBeInTheDocument();
        expect(screen.getAllByText("demo-tool")).toHaveLength(1);
        expect(screen.getAllByTestId("shine-border")).toHaveLength(1);
    });
});
