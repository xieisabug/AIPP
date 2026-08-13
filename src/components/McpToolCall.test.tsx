import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import McpToolCall, { ToolErrorContinueProvider } from "@/components/McpToolCall";
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

    it("keeps a successful result when a later success update omits result", async () => {
        const conversationId = 18;
        const callId = 152;
        const resultText = "{\"items\":[\"search-result\"]}";

        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: callId,
            conversation_id: conversationId,
            message_id: 22,
            server_id: 1,
            server_name: "search",
            tool_name: "search_web",
            parameters: "{\"query\":\"hello\"}",
            status: "success",
            result: resultText,
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const firstState = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "success",
                server_name: "search",
                tool_name: "search_web",
                parameters: "{\"query\":\"hello\"}",
                result: resultText,
            }],
        ]);

        const { rerender } = render(
            <McpToolCall
                conversationId={conversationId}
                messageId={22}
                callId={callId}
                serverName="search"
                toolName="search_web"
                parameters='{"query":"hello"}'
                mcpToolCallStates={firstState}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();
        expect(screen.queryByText(/search-result/)).not.toBeInTheDocument();
        await userEvent.click(screen.getByTitle("展开详情"));
        expect(screen.getByText(/search-result/)).toBeInTheDocument();
        await userEvent.click(screen.getByTitle("收起详情"));
        expect(screen.queryByText(/search-result/)).not.toBeInTheDocument();

        const laterState = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "success",
                server_name: "search",
                tool_name: "search_web",
                parameters: "{\"query\":\"hello\"}",
            }],
        ]);

        rerender(
            <McpToolCall
                conversationId={conversationId}
                messageId={22}
                callId={callId}
                serverName="search"
                toolName="search_web"
                parameters='{"query":"hello"}'
                mcpToolCallStates={laterState}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();
        expect(screen.queryByText(/search-result/)).not.toBeInTheDocument();
        await userEvent.click(screen.getByTitle("展开详情"));
        expect(screen.getByText(/search-result/)).toBeInTheDocument();
    });

    it("renders streaming state with '生成中' badge and shine border", async () => {
        const conversationId = 9;

        render(
            <McpToolCall
                conversationId={conversationId}
                messageId={14}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"partial":"args"}'
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
                isStreaming={true}
            />
        );

        await flushEffects();

        // Should show "生成中" badge
        expect(screen.getByText("生成中")).toBeInTheDocument();
        // Streaming state should show shine border
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
        // Should display server and tool name
        expect(screen.getByText("demo-server")).toBeInTheDocument();
        expect(screen.getByText("demo-tool")).toBeInTheDocument();
    });

    it("does not show execute button in streaming state", async () => {
        const conversationId = 10;

        render(
            <McpToolCall
                conversationId={conversationId}
                messageId={15}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{}'
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
                isStreaming={true}
            />
        );

        await flushEffects();

        // No execute/play button should be visible
        expect(screen.queryByTitle("执行")).not.toBeInTheDocument();
        expect(screen.queryByTitle("重新执行")).not.toBeInTheDocument();
        expect(screen.queryByTitle("停止")).not.toBeInTheDocument();
    });

    it("binds a streaming placeholder to MCP state via llm_call_id", async () => {
        const conversationId = 11;
        const callId = 88;
        const llmCallId = "call_stream_88";

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "executing",
                llm_call_id: llmCallId,
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"bound-from-state\"}",
            }],
        ]);

        render(
            <McpToolCall
                conversationId={conversationId}
                messageId={16}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"query":"partial"}'
                llmCallId={llmCallId}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={callId}
                isStreaming={true}
            />
        );

        await flushEffects();

        expect(screen.getByText("执行中")).toBeInTheDocument();
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
        expect(screen.getByText(/bound-from-state/)).toBeInTheDocument();
    });

    it("auto-collapses failed calls and hides failed actions when tool error continuation is enabled", async () => {
        const conversationId = 19;
        const callId = 162;
        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: callId,
            conversation_id: conversationId,
            message_id: 23,
            server_id: 1,
            server_name: "demo-server",
            tool_name: "demo-tool",
            parameters: "{\"query\":\"hello\"}",
            status: "failed",
            error: "boom",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "failed",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
                error: "boom",
            }],
        ]);

        render(
            <McpToolCall
                conversationId={conversationId}
                messageId={23}
                callId={callId}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"query":"hello"}'
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();

        expect(screen.getByText("失败")).toBeInTheDocument();
        expect(screen.getByTitle("展开详情")).toBeInTheDocument();
        expect(screen.queryByTitle("重新执行")).not.toBeInTheDocument();
        expect(screen.queryByTitle("以错误继续对话")).not.toBeInTheDocument();
        expect(screen.queryByText("重新执行")).not.toBeInTheDocument();
        expect(screen.queryByText("以错误继续")).not.toBeInTheDocument();
    });

    it("does not auto-collapse a failed call after the user manually expands it", async () => {
        const user = userEvent.setup();
        const conversationId = 21;
        const callId = 182;
        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: callId,
            conversation_id: conversationId,
            message_id: 25,
            server_id: 1,
            server_name: "demo-server",
            tool_name: "demo-tool",
            parameters: "{\"query\":\"hello\"}",
            status: "failed",
            error: "boom",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const failedState = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "failed",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
                error: "boom",
            }],
        ]);

        const { rerender } = render(
            <McpToolCall
                conversationId={conversationId}
                messageId={25}
                callId={callId}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"query":"hello"}'
                mcpToolCallStates={failedState}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();

        expect(screen.getByTitle("展开详情")).toBeInTheDocument();
        await user.click(screen.getByTitle("展开详情"));
        expect(screen.getByTitle("收起详情")).toBeInTheDocument();
        expect(screen.getByText(/boom/)).toBeInTheDocument();

        const repeatedFailedState = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "failed",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
                error: "boom",
            }],
        ]);

        rerender(
            <McpToolCall
                conversationId={conversationId}
                messageId={25}
                callId={callId}
                serverName="demo-server"
                toolName="demo-tool"
                parameters='{"query":"hello"}'
                mcpToolCallStates={repeatedFailedState}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();

        expect(screen.getByTitle("收起详情")).toBeInTheDocument();
        expect(screen.getByText(/boom/)).toBeInTheDocument();
    });

    it("keeps failed calls expanded with failed actions when tool error continuation is disabled", async () => {
        const conversationId = 20;
        const callId = 172;
        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: callId,
            conversation_id: conversationId,
            message_id: 24,
            server_id: 1,
            server_name: "demo-server",
            tool_name: "demo-tool",
            parameters: "{\"query\":\"hello\"}",
            status: "failed",
            error: "boom",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "failed",
                server_name: "demo-server",
                tool_name: "demo-tool",
                parameters: "{\"query\":\"hello\"}",
                error: "boom",
            }],
        ]);

        render(
            <ToolErrorContinueProvider value={false}>
                <McpToolCall
                    conversationId={conversationId}
                    messageId={24}
                    callId={callId}
                    serverName="demo-server"
                    toolName="demo-tool"
                    parameters='{"query":"hello"}'
                    mcpToolCallStates={mcpToolCallStates}
                    shiningMcpCallId={null}
                />
            </ToolErrorContinueProvider>
        );

        await flushEffects();

        expect(screen.getByText("失败")).toBeInTheDocument();
        expect(screen.getByTitle("收起详情")).toBeInTheDocument();
        expect(screen.getByText("重新执行")).toBeInTheDocument();
        expect(screen.getByText("以错误继续")).toBeInTheDocument();
    });

    it("renders protocol-level failures without execution actions when no call_id exists", async () => {
        render(
            <McpToolCall
                conversationId={31}
                messageId={41}
                serverName="default"
                toolName="load_skill"
                parameters='{"command":"skill-creator"}'
                llmCallId="call_setup_failed"
                status="failed"
                error="服务器 'default' 未找到或已禁用"
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        await flushEffects();

        expect(screen.getByText("失败")).toBeInTheDocument();
        expect(screen.getByText(/服务器 'default' 未找到或已禁用/)).toBeInTheDocument();
        expect(screen.getByTitle("收起详情")).toBeInTheDocument();
        expect(screen.queryByTitle("执行")).not.toBeInTheDocument();
        expect(screen.queryByTitle("重新执行")).not.toBeInTheDocument();
        expect(screen.queryByTitle("以错误继续对话")).not.toBeInTheDocument();
        expect(screen.queryByText("重新执行")).not.toBeInTheDocument();
        expect(screen.queryByText("以错误继续")).not.toBeInTheDocument();
    });
});
