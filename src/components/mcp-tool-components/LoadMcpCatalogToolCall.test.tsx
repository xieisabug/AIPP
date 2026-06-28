import { render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import LoadMcpCatalogToolCall from "@/components/mcp-tool-components/LoadMcpCatalogToolCall";
import { ToolErrorContinueProvider } from "@/components/McpToolCall";
import { clearAllMockHandlers } from "@/__tests__/mocks/tauri";

vi.mock("@/contexts/AntiLeakageContext", () => ({
    useAntiLeakage: () => ({
        enabled: false,
        isRevealed: true,
    }),
}));

vi.mock("@/components/magicui/shine-border", () => ({
    ShineBorder: () => <div data-testid="shine-border" />,
}));

describe("LoadMcpCatalogToolCall", () => {
    beforeEach(() => {
        clearAllMockHandlers();
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("renders executing state with shine border and stop action", () => {
        const states = new Map([
            [20, {
                call_id: 20,
                conversation_id: 1,
                message_id: 2,
                status: "executing" as const,
                server_name: "aipp:agent",
                tool_name: "load_mcp_tool",
                parameters: '{"server_name":"search","names":["fetch_url"]}',
            }],
        ]);

        render(
            <LoadMcpCatalogToolCall
                kind="tool"
                callId={20}
                conversationId={1}
                messageId={2}
                serverName="aipp:agent"
                toolName="load_mcp_tool"
                parameters='{"server_name":"search","names":["fetch_url"]}'
                status="executing"
                mcpToolCallStates={states}
                shiningMcpCallId={20}
            />,
        );

        expect(screen.getByText("加载中")).toBeInTheDocument();
        expect(screen.getByText("加载工具")).toBeInTheDocument();
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
        expect(screen.getByTitle("停止")).toBeInTheDocument();
    });

    it("shows failure reason and preserves retry visibility when auto-continue is disabled", () => {
        const states = new Map([
            [21, {
                call_id: 21,
                conversation_id: 1,
                message_id: 2,
                status: "failed" as const,
                server_name: "aipp:agent",
                tool_name: "load_mcp_server",
                parameters: '{"name":"browser"}',
                error: "server not found",
            }],
        ]);

        render(
            <ToolErrorContinueProvider value={false}>
                <LoadMcpCatalogToolCall
                    kind="server"
                    callId={21}
                    conversationId={1}
                    messageId={2}
                    serverName="aipp:agent"
                    toolName="load_mcp_server"
                    parameters='{"name":"browser"}'
                    status="failed"
                    mcpToolCallStates={states}
                />
            </ToolErrorContinueProvider>,
        );

        expect(screen.getByText("加载失败")).toBeInTheDocument();
        expect(screen.getByText("错误原因：server not found")).toBeInTheDocument();
        expect(screen.getByTitle("重新执行")).toBeInTheDocument();
    });
});
