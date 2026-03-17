import { act, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import McpToolCall from "@/components/McpToolCall";
import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/contexts/AntiLeakageContext", () => ({
    useAntiLeakage: () => ({
        enabled: false,
        isRevealed: true,
    }),
}));

vi.mock("@/components/magicui/shine-border", () => ({
    ShineBorder: () => <div data-testid="shine-border" />,
}));

const flushEffects = async () => {
    await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
    });
};

describe("McpToolCall call_id binding", () => {
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

    it("does not guess an existing tool call when streamed call_id is still missing", async () => {
        const conversationId = 7;
        const guessedCallId = 41;

        mockInvokeHandler("get_mcp_tool_calls_by_conversation", () => [
            {
                id: guessedCallId,
                conversation_id: conversationId,
                message_id: 10,
                server_id: 1,
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
                status: "executing",
                created_time: "2024-01-01T00:00:00.000Z",
            },
        ]);

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [guessedCallId, {
                call_id: guessedCallId,
                conversation_id: conversationId,
                status: "executing",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
            }],
        ]);

        render(
            <McpToolCall
                conversationId={conversationId}
                messageId={10}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"query":"hello"}'
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={guessedCallId}
            />
        );

        await flushEffects();

        expect(invoke).not.toHaveBeenCalledWith(
            "get_mcp_tool_calls_by_conversation",
            { conversationId }
        );
        expect(screen.queryByText("执行中")).not.toBeInTheDocument();
        expect(screen.queryByTestId("shine-border")).not.toBeInTheDocument();
    });

    it("renders pending state without showing a shine border", async () => {
        const conversationId = 8;
        const callId = 52;

        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: callId,
            conversation_id: conversationId,
            message_id: 12,
            server_id: 1,
            server_name: "demo-server",
            tool_name: "demo-tool",
            parameters: "{\"query\":\"hello\"}",
            status: "pending",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "pending",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
            }],
        ]);

        render(
            <McpToolCall
                conversationId={conversationId}
                messageId={12}
                callId={callId}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"query":"hello"}'
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();

        expect(screen.getByText("待执行")).toBeInTheDocument();
        expect(screen.queryByTestId("shine-border")).not.toBeInTheDocument();
    });
});
