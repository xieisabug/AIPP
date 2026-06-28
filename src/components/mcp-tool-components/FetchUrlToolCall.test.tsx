import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import FetchUrlToolCall from "@/components/mcp-tool-components/FetchUrlToolCall";
import { ToolErrorContinueProvider } from "@/components/McpToolCall";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

const antiLeakageState = vi.hoisted(() => ({
    enabled: false,
    isRevealed: true,
}));

vi.mock("@/contexts/AntiLeakageContext", () => ({
    useAntiLeakage: () => antiLeakageState,
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

const baseProps: McpToolComponentProps = {
    serverName: "aipp:search",
    toolName: "fetch_url",
    parameters: '{"url":"https://example.com/article"}',
    conversationId: 1,
    messageId: 2,
};

describe("FetchUrlToolCall", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        antiLeakageState.enabled = false;
        antiLeakageState.isRevealed = true;
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("renders idle state with target url", () => {
        render(<FetchUrlToolCall {...baseProps} />);
        expect(screen.getByText("https://example.com/article")).toBeInTheDocument();
        expect(screen.getByText("抓取网页")).toBeInTheDocument();
    });

    it("renders executing state with loading indicator", () => {
        const states = new Map([
            [11, {
                call_id: 11,
                conversation_id: 1,
                message_id: 2,
                status: "executing" as const,
                server_name: "aipp:search",
                tool_name: "fetch_url",
                parameters: '{"url":"https://example.com/article"}',
            }],
        ]);
        render(
            <FetchUrlToolCall
                {...baseProps}
                callId={11}
                status="executing"
                mcpToolCallStates={states}
                shiningMcpCallId={11}
            />,
        );
        expect(screen.getByText("抓取中")).toBeInTheDocument();
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
    });

    it("renders success state with word count and elapsed time", () => {
        const result = JSON.stringify([{ type: "text", text: "Hello world. 你好世界。" }]);
        const states = new Map([
            [11, {
                call_id: 11,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "fetch_url",
                parameters: '{"url":"https://example.com/article"}',
                result,
                started_time: new Date("2026-06-20T01:00:00.000Z"),
                finished_time: new Date("2026-06-20T01:00:01.234Z"),
            }],
        ]);
        render(<FetchUrlToolCall {...baseProps} callId={11} status="success" mcpToolCallStates={states} />);
        expect(screen.getByText("抓取完成")).toBeInTheDocument();
        expect(screen.getByText("6 字")).toBeInTheDocument();
        expect(screen.getByTitle("参考文档：6 字")).toBeInTheDocument();
        expect(screen.getByText("1.2 秒")).toBeInTheDocument();
        expect(screen.getByTitle("抓取耗时：1.2 秒")).toBeInTheDocument();
        expect(screen.queryByText(/字符/)).not.toBeInTheDocument();
    });

    it("opens sidebar and emits focus event when card body is clicked in success state", async () => {
        const user = userEvent.setup();
        const result = JSON.stringify({
            content: [{ type: "text", text: "Hello world" }],
            isError: false,
        });
        const states = new Map([
            [11, {
                call_id: 11,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "fetch_url",
                parameters: '{"url":"https://example.com/article"}',
                result,
            }],
        ]);
        const { container } = render(
            <FetchUrlToolCall {...baseProps} callId={11} status="success" mcpToolCallStates={states} />,
        );
        const card = container.querySelector('[role="button"]');
        expect(card).not.toBeNull();
        await user.click(card!);

        expect(emit).toHaveBeenCalledWith("sidebar-focus-context", { id: "mcp-11" });
    });

    it("uses url text click to focus the sidebar instead of opening the browser", async () => {
        const user = userEvent.setup();
        const result = JSON.stringify({
            content: [{ type: "text", text: "Hello world" }],
            isError: false,
        });
        const states = new Map([
            [11, {
                call_id: 11,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "fetch_url",
                parameters: '{"url":"https://example.com/article"}',
                result,
            }],
        ]);
        render(<FetchUrlToolCall {...baseProps} callId={11} status="success" mcpToolCallStates={states} />);
        await user.click(screen.getByText("https://example.com/article"));
        expect(emit).toHaveBeenCalledWith("sidebar-focus-context", { id: "mcp-11" });
    });

    it("renders failed state with error message", () => {
        render(<FetchUrlToolCall {...baseProps} status="failed" error="network error" />);
        expect(screen.getByText("抓取失败")).toBeInTheDocument();
        expect(screen.getByText("network error")).toBeInTheDocument();
    });

    it("shows failed recovery controls when a real call id exists and auto-continue is disabled", () => {
        const states = new Map([
            [11, {
                call_id: 11,
                conversation_id: 1,
                message_id: 2,
                status: "failed" as const,
                server_name: "aipp:search",
                tool_name: "fetch_url",
                parameters: '{"url":"https://example.com/article"}',
                error: "network error",
            }],
        ]);
        render(
            <ToolErrorContinueProvider value={false}>
                <FetchUrlToolCall {...baseProps} callId={11} status="failed" mcpToolCallStates={states} />
            </ToolErrorContinueProvider>,
        );

        expect(screen.getByTitle("重新执行")).toBeInTheDocument();
        expect(screen.getByTitle("以错误继续对话")).toBeInTheDocument();
    });

    it("masks url tooltip while anti-leakage is enabled and hidden", () => {
        antiLeakageState.enabled = true;
        antiLeakageState.isRevealed = false;

        render(<FetchUrlToolCall {...baseProps} />);

        expect(screen.getByText("******")).toHaveAttribute("title", "******");
        expect(screen.queryByTitle("https://example.com/article（点击在外部打开）")).not.toBeInTheDocument();
    });

    it("executes tool call when play button is clicked", async () => {
        const user = userEvent.setup();
        mockInvokeHandler("create_mcp_tool_call", () => ({
            id: 11,
            conversation_id: 1,
            message_id: 2,
            server_id: 1,
            server_name: "aipp:search",
            tool_name: "fetch_url",
            parameters: '{"url":"https://example.com/article"}',
            status: "executing",
            created_time: "2024-01-01T00:00:00.000Z",
        }));
        mockInvokeHandler("execute_mcp_tool_call", () => ({
            id: 11,
            conversation_id: 1,
            message_id: 2,
            server_id: 1,
            server_name: "aipp:search",
            tool_name: "fetch_url",
            parameters: '{"url":"https://example.com/article"}',
            status: "success",
            result: JSON.stringify({ content: [{ type: "text", text: "OK" }], isError: false }),
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        render(<FetchUrlToolCall {...baseProps} />);
        const playButton = screen.getByTitle("执行");
        await user.click(playButton);

        await flushEffects();

        expect(invoke).toHaveBeenCalledWith("create_mcp_tool_call", expect.any(Object));
        expect(invoke).toHaveBeenCalledWith("execute_mcp_tool_call", expect.any(Object));
    });
});
