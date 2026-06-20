import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";
import SearchWebToolCall from "@/components/mcp-tool-components/SearchWebToolCall";
import { ToolErrorContinueProvider } from "@/components/McpToolCall";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
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

const baseProps: McpToolComponentProps = {
    serverName: "aipp:search",
    toolName: "search_web",
    parameters: '{"query":"Rust async"}',
    conversationId: 1,
    messageId: 2,
};

describe("SearchWebToolCall", () => {
    beforeEach(() => {
        clearAllMockHandlers();
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("renders idle state with query keyword", () => {
        render(<SearchWebToolCall {...baseProps} />);
        expect(screen.getByText("Rust async")).toBeInTheDocument();
        expect(screen.getByText("网络搜索")).toBeInTheDocument();
    });

    it("renders executing state with loading indicator and stop button", () => {
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "executing" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async"}',
            }],
        ]);
        render(
            <SearchWebToolCall
                {...baseProps}
                callId={10}
                status="executing"
                mcpToolCallStates={states}
                shiningMcpCallId={10}
            />,
        );
        expect(screen.getByText("搜索中")).toBeInTheDocument();
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
        expect(screen.getByTitle("停止")).toBeInTheDocument();
    });

    it("renders success state with engine and elapsed time", () => {
        const result = JSON.stringify({
            content: [{ type: "text", text: "# Results\n\n1. Rust Lang" }],
            isError: false,
            search_engine: "Google",
            search_time_ms: 250,
        });
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async"}',
                result,
            }],
        ]);
        render(<SearchWebToolCall {...baseProps} callId={10} status="success" mcpToolCallStates={states} />);
        expect(screen.getByText("搜索完成")).toBeInTheDocument();
        expect(screen.getByText("Google")).toBeInTheDocument();
        expect(screen.getByText("0.3 秒")).toBeInTheDocument();
    });

    it("estimates result count from markdown by counting distinct external links", () => {
        const markdown = [
            "# Search Results",
            "",
            "[Rust Lang](https://rust-lang.org)",
            "Official site",
            "",
            "[Rust docs](https://doc.rust-lang.org)",
            "Learn Rust",
            "",
            "[Rust Lang](https://rust-lang.org)",
            "Duplicate link should be deduped",
            "",
            "[Nav](/internal)",
            "Relative URL should be ignored",
        ].join("\n");
        const result = JSON.stringify({
            content: [{ type: "text", text: markdown }],
            isError: false,
            search_engine: "Google",
            search_time_ms: 250,
        });
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async"}',
                result,
            }],
        ]);
        render(<SearchWebToolCall {...baseProps} callId={10} status="success" mcpToolCallStates={states} />);
        // 2 distinct external URLs (rust-lang.org, doc.rust-lang.org); the
        // duplicate and the relative URL are both excluded.
        expect(screen.getByText("2 条")).toBeInTheDocument();
    });

    it("renders result count and elapsed time for items response", () => {
        const result = JSON.stringify([{
            type: "json",
            json: {
                items: [
                    { title: "A", url: "https://a.com", snippet: "", rank: 1 },
                    { title: "B", url: "https://b.com", snippet: "", rank: 2 },
                ],
            },
        }]);
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async","result_type":"items"}',
                result,
                started_time: new Date("2026-06-20T01:00:00.000Z"),
                finished_time: new Date("2026-06-20T01:00:00.456Z"),
            }],
        ]);
        render(<SearchWebToolCall {...baseProps} callId={10} status="success" mcpToolCallStates={states} />);
        expect(screen.getByText("2 条")).toBeInTheDocument();
        expect(screen.getByText("0.5 秒")).toBeInTheDocument();
    });

    it("renders result count for items-only array response", () => {
        const result = JSON.stringify([{
            type: "json",
            json: [
                { title: "A", url: "https://a.com", snippet: "", rank: 1 },
                { title: "B", url: "https://b.com", snippet: "", rank: 2 },
                { title: "C", url: "https://c.com", snippet: "", rank: 3 },
            ],
        }]);
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async","result_type":"items"}',
                result,
                started_time: new Date("2026-06-20T01:00:00.000Z"),
                finished_time: new Date("2026-06-20T01:00:01.000Z"),
            }],
        ]);
        render(<SearchWebToolCall {...baseProps} callId={10} status="success" mcpToolCallStates={states} />);
        expect(screen.getByText("3 条")).toBeInTheDocument();
        expect(screen.getByText("1 秒")).toBeInTheDocument();
        expect(screen.queryByText("1 条")).not.toBeInTheDocument();
    });

    it("opens sidebar and emits focus event when card is clicked in success state", async () => {
        const user = userEvent.setup();
        const result = JSON.stringify({
            content: [{ type: "text", text: "# Results" }],
            isError: false,
            search_engine: "Google",
            search_time_ms: 250,
        });
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "success" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async"}',
                result,
            }],
        ]);
        const { container } = render(
            <SearchWebToolCall {...baseProps} callId={10} status="success" mcpToolCallStates={states} />,
        );
        const card = container.querySelector('[role="button"]');
        expect(card).not.toBeNull();
        await user.click(card!);

        expect(emit).toHaveBeenCalledWith("sidebar-focus-context", { id: "mcp-10" });
    });

    it("does not emit focus event before reaching success state", async () => {
        const user = userEvent.setup();
        const { container } = render(<SearchWebToolCall {...baseProps} />);
        // No role="button" because card is not focusable before success
        expect(container.querySelector('[role="button"]')).toBeNull();
        // Clicking the card area should not emit anything
        const card = container.firstChild as HTMLElement;
        await user.click(card);
        expect(emit).not.toHaveBeenCalled();
    });

    it("renders failed state with error message", () => {
        render(<SearchWebToolCall {...baseProps} status="failed" error="timeout" />);
        expect(screen.getByText("搜索失败")).toBeInTheDocument();
        expect(screen.getByText("timeout")).toBeInTheDocument();
    });

    it("shows failed recovery controls when a real call id exists and auto-continue is disabled", () => {
        const states = new Map([
            [10, {
                call_id: 10,
                conversation_id: 1,
                message_id: 2,
                status: "failed" as const,
                server_name: "aipp:search",
                tool_name: "search_web",
                parameters: '{"query":"Rust async"}',
                error: "timeout",
            }],
        ]);
        render(
            <ToolErrorContinueProvider value={false}>
                <SearchWebToolCall {...baseProps} callId={10} status="failed" mcpToolCallStates={states} />
            </ToolErrorContinueProvider>,
        );

        expect(screen.getByTitle("重新执行")).toBeInTheDocument();
        expect(screen.getByTitle("以错误继续对话")).toBeInTheDocument();
    });

    it("executes tool call when play button is clicked", async () => {
        const user = userEvent.setup();
        mockInvokeHandler("create_mcp_tool_call", () => ({
            id: 10,
            conversation_id: 1,
            message_id: 2,
            server_id: 1,
            server_name: "aipp:search",
            tool_name: "search_web",
            parameters: '{"query":"Rust async"}',
            status: "executing",
            created_time: "2024-01-01T00:00:00.000Z",
        }));
        mockInvokeHandler("execute_mcp_tool_call", () => ({
            id: 10,
            conversation_id: 1,
            message_id: 2,
            server_id: 1,
            server_name: "aipp:search",
            tool_name: "search_web",
            parameters: '{"query":"Rust async"}',
            status: "success",
            result: JSON.stringify({ content: [], isError: false }),
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        render(<SearchWebToolCall {...baseProps} />);
        const playButton = screen.getByTitle("执行");
        await user.click(playButton);

        await flushEffects();

        expect(invoke).toHaveBeenCalledWith("create_mcp_tool_call", expect.any(Object));
        expect(invoke).toHaveBeenCalledWith("execute_mcp_tool_call", expect.any(Object));
    });
});
