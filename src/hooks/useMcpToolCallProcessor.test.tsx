import React from "react";
import { render, screen, waitFor } from "@testing-library/react";
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

vi.mock("@/hooks/useDisplayConfig", () => ({
    useDisplayConfig: () => ({
        config: null,
        isLoading: false,
        error: null,
        isUserMessageMarkdownEnabled: true,
        isMergeAssistantMessages: true,
        isShowThinking: true,
        isPreviewCodeShowToolbar: true,
        refreshConfig: vi.fn(),
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

    it("renders streaming tool call markers with '生成中' badge", async () => {
        const conversationId = 22;

        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: 0,
            conversation_id: conversationId,
            message_id: 12,
            server_id: 1,
            server_name: "my-server",
            tool_name: "my-tool",
            parameters: '{}',
            status: "pending",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const markdown =
            'Some text before\n\n<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"my-server","tool_name":"my-tool","fn_arguments":"{\\"key\\":\\"value\\"}","llm_call_id":"call_1"} -->\n';

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={12}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />,
        );

        // Should render the text before the marker
        expect(screen.getByText("Some text before")).toBeInTheDocument();
        // Should show "生成中" badge for streaming tool call
        expect(await screen.findByText("生成中")).toBeInTheDocument();
        // Should show the server and tool name
        expect(screen.getByText("my-server")).toBeInTheDocument();
        expect(screen.getByText("my-tool")).toBeInTheDocument();
        // Should show shine border for streaming
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
    });

    it("renders multiple streaming tool calls", async () => {
        const conversationId = 23;

        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: 0,
            conversation_id: conversationId,
            message_id: 13,
            server_id: 1,
            server_name: "server",
            tool_name: "tool",
            parameters: '{}',
            status: "pending",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const markdown = [
            '<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"server-a","tool_name":"tool-a","fn_arguments":"{}","llm_call_id":"call_a"} -->',
            '<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"server-b","tool_name":"tool-b","fn_arguments":"{}","llm_call_id":"call_b"} -->',
        ].join("\n");

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={13}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />,
        );

        expect(screen.getAllByText("生成中")).toHaveLength(2);
        expect(screen.getByText("tool-a")).toBeInTheDocument();
        expect(screen.getByText("tool-b")).toBeInTheDocument();
    });

    it("does not confuse MCP_TOOL_CALL_STREAMING with MCP_TOOL_CALL", async () => {
        const conversationId = 24;
        const callId = 301;

        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: callId,
            conversation_id: conversationId,
            message_id: 14,
            server_id: 1,
            server_name: "server",
            tool_name: "tool",
            parameters: '{}',
            status: "pending",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [callId, {
                call_id: callId,
                conversation_id: conversationId,
                status: "pending",
                server_name: "real-server",
                tool_name: "real-tool",
                parameters: '{}',
            }],
        ]);

        const markdown = [
            '<!-- MCP_TOOL_CALL:{"call_id":301,"server_name":"real-server","tool_name":"real-tool","parameters":"{}"} -->',
            '<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"streaming-server","tool_name":"streaming-tool","fn_arguments":"{}","llm_call_id":"call_2"} -->',
        ].join("\n");

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={14}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={null}
            />,
        );

        // Real tool call should show "待执行" (pending)
        expect(await screen.findByText("待执行")).toBeInTheDocument();
        // Streaming tool call should show "生成中"
        expect(screen.getByText("生成中")).toBeInTheDocument();
        // Both tool names should be present
        expect(screen.getByText("real-tool")).toBeInTheDocument();
        expect(screen.getByText("streaming-tool")).toBeInTheDocument();
    });

    it("transitions from streaming marker to real MCP_TOOL_CALL on rerender", async () => {
        const conversationId = 25;
        const resolvedCallId = 401;

        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: resolvedCallId,
            conversation_id: conversationId,
            message_id: 15,
            server_id: 1,
            server_name: "my-server",
            tool_name: "my-tool",
            parameters: '{"key":"value"}',
            status: "pending",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        // Phase 1: Streaming marker (LLM is generating arguments)
        const llmCallId = "call_x";
        const streamingMarkdown =
            `Hello\n\n<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"my-server","tool_name":"my-tool","fn_arguments":"{\\"key\\":\\"val","llm_call_id":"${llmCallId}"} -->\n`;

        const { rerender } = render(
            <ProcessorHarness
                markdown={streamingMarkdown}
                conversationId={conversationId}
                messageId={15}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />,
        );

        // Should show streaming state
        expect(await screen.findByText("生成中")).toBeInTheDocument();
        expect(screen.getByText("my-tool")).toBeInTheDocument();
        expect(screen.getAllByText("my-tool")).toHaveLength(1);

        // Phase 2: Real MCP_TOOL_CALL marker replaces streaming
        const finalMarkdown =
            `Hello\n\n<!-- MCP_TOOL_CALL:{"call_id":401,"server_name":"my-server","tool_name":"my-tool","parameters":"{\\"key\\":\\"value\\"}","llm_call_id":"${llmCallId}"} -->\n`;

        const mcpStates = new Map<number, MCPToolCallUpdateEvent>([
            [resolvedCallId, {
                call_id: resolvedCallId,
                conversation_id: conversationId,
                status: "pending",
                server_name: "my-server",
                tool_name: "my-tool",
                parameters: '{"key":"value"}',
            }],
        ]);

        rerender(
            <ProcessorHarness
                markdown={finalMarkdown}
                conversationId={conversationId}
                messageId={15}
                mcpToolCallStates={mcpStates}
                shiningMcpCallId={null}
            />,
        );

        // Should now show "待执行" instead of "生成中"
        expect(await screen.findByText("待执行")).toBeInTheDocument();
        expect(screen.queryByText("生成中")).not.toBeInTheDocument();
        expect(screen.getByText("my-tool")).toBeInTheDocument();
        expect(screen.getAllByText("my-tool")).toHaveLength(1);
    });

    it("binds streaming markers to executing MCP state through llm_call_id before final marker arrives", async () => {
        const conversationId = 26;
        const resolvedCallId = 501;
        const llmCallId = "call_bind_501";

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [resolvedCallId, {
                call_id: resolvedCallId,
                conversation_id: conversationId,
                status: "executing",
                llm_call_id: llmCallId,
                server_name: "my-server",
                tool_name: "my-tool",
                parameters: '{"query":"bound-from-state"}',
            }],
        ]);

        const markdown =
            `Hello\n\n<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"my-server","tool_name":"my-tool","fn_arguments":"{\\"query\\":\\"partial\\"}","llm_call_id":"${llmCallId}"} -->\n`;

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={16}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={resolvedCallId}
            />
        );

        expect(await screen.findByText("执行中")).toBeInTheDocument();
        expect(screen.getByText(/bound-from-state/)).toBeInTheDocument();
        expect(screen.getByTestId("shine-border")).toBeInTheDocument();
    });

    it("renders protocol-level failed MCP comments without execution actions", async () => {
        const markdown = `<!-- MCP_TOOL_CALL:${JSON.stringify({
            server_name: "default",
            tool_name: "load_skill",
            parameters: '{"command":"skill-creator"}',
            llm_call_id: "call_failed",
            status: "failed",
            error: "服务器 'default' 未找到或已禁用",
        })} -->`;

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={32}
                messageId={23}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        expect(await screen.findByText("失败")).toBeInTheDocument();
        expect(screen.getByText(/服务器 'default' 未找到或已禁用/)).toBeInTheDocument();
        expect(screen.queryByTitle("执行")).not.toBeInTheDocument();
        expect(screen.queryByText("重新执行")).not.toBeInTheDocument();
        expect(screen.queryByText("以错误继续")).not.toBeInTheDocument();
    });

    it("maps legacy setup_error comments to the unified failed protocol state", async () => {
        const markdown = `<!-- MCP_TOOL_CALL:${JSON.stringify({
            server_name: "default",
            tool_name: "load_skill",
            parameters: "{}",
            llm_call_id: "call_legacy_failed",
            setup_error: "legacy setup failed",
        })} -->`;

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={33}
                messageId={24}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        expect(await screen.findByText("失败")).toBeInTheDocument();
        expect(screen.getByText(/legacy setup failed/)).toBeInTheDocument();
        expect(screen.queryByTitle("执行")).not.toBeInTheDocument();
    });

    it("renders preview_code streaming cards inline", async () => {
        const conversationId = 27;
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        const markdown =
            '<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"ui_interaction","tool_name":"preview_code","fn_arguments":"{\\"title\\":\\"compound_interest\\",\\"renderer\\":\\"html\\",\\"code\\":\\"<div>Loading</div>\\",\\"loading_messages\\":[\\"正在生成交互面板\\"]}","llm_call_id":"preview_call_1"} -->';

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={18}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        expect(await screen.findByText("compound_interest")).toBeInTheDocument();
        expect(screen.getByText("正在生成交互面板")).toBeInTheDocument();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Loading"));
    });

    it("keeps preview_code streaming markup when MCP state is already executing", async () => {
        const conversationId = 28;
        const resolvedCallId = 601;
        const llmCallId = "preview_call_2";
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [resolvedCallId, {
                call_id: resolvedCallId,
                conversation_id: conversationId,
                status: "executing",
                llm_call_id: llmCallId,
                server_name: "ui_interaction",
                tool_name: "preview_code",
                parameters: JSON.stringify({
                    title: "compound_interest",
                    renderer: "html",
                    code: "<div>Final Content</div>",
                    loading_messages: ["正在生成交互面板"],
                }),
            }],
        ]);

        const markdown =
            `<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"ui_interaction","tool_name":"preview_code","fn_arguments":"{\\"title\\":\\"compound_interest\\",\\"renderer\\":\\"html\\",\\"code\\":\\"<div>Live Content</div>\\",\\"loading_messages\\":[\\"正在生成交互面板\\"]}","llm_call_id":"${llmCallId}"} -->`;

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={19}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={resolvedCallId}
            />
        );

        expect(screen.queryByText("等待交互")).not.toBeInTheDocument();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Live Content"));
        expect(host.shadowRoot?.textContent).not.toContain("Final Content");
    });

    it("keeps the same preview_code host when streaming markup settles into a persisted tool call", async () => {
        const conversationId = 31;
        const messageId = 22;
        const resolvedCallId = 701;
        const llmCallId = "preview_call_stable_key";
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        mockInvokeHandler("get_mcp_tool_call", () => ({
            id: resolvedCallId,
            conversation_id: conversationId,
            message_id: messageId,
            server_id: 1,
            server_name: "ui_interaction",
            tool_name: "preview_code",
            parameters: JSON.stringify({
                title: "stable_preview",
                renderer: "html",
                code: "<div>Final Content</div>",
                loading_messages: ["正在生成交互面板"],
            }),
            status: "executing",
            created_time: "2024-01-01T00:00:00.000Z",
        }));

        const { rerender } = render(
            <ProcessorHarness
                markdown={`<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"ui_interaction","tool_name":"preview_code","fn_arguments":"{\\"title\\":\\"stable_preview\\",\\"renderer\\":\\"html\\",\\"code\\":\\"<div>Live Content</div>\\",\\"loading_messages\\":[\\"正在生成交互面板\\"]}","llm_call_id":"${llmCallId}"} -->`}
                conversationId={conversationId}
                messageId={messageId}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        const streamingHost = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(streamingHost.shadowRoot?.textContent).toContain("Live Content"));

        const mcpToolCallStates = new Map<number, MCPToolCallUpdateEvent>([
            [resolvedCallId, {
                call_id: resolvedCallId,
                conversation_id: conversationId,
                status: "executing",
                llm_call_id: llmCallId,
                server_name: "ui_interaction",
                tool_name: "preview_code",
                parameters: JSON.stringify({
                    title: "stable_preview",
                    renderer: "html",
                    code: "<div>Final Content</div>",
                    loading_messages: ["正在生成交互面板"],
                }),
            }],
        ]);

        rerender(
            <ProcessorHarness
                markdown={`<!-- MCP_TOOL_CALL:{"call_id":${resolvedCallId},"llm_call_id":"${llmCallId}","server_name":"ui_interaction","tool_name":"preview_code","parameters":"{\\"title\\":\\"stable_preview\\",\\"renderer\\":\\"html\\",\\"code\\":\\"<div>Final Content</div>\\",\\"loading_messages\\":[\\"正在生成交互面板\\"]}"} -->`}
                conversationId={conversationId}
                messageId={messageId}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={resolvedCallId}
            />
        );

        const finalHost = await screen.findByTestId("preview-code-host");
        expect(finalHost).toBe(streamingHost);
        await waitFor(() => expect(finalHost.shadowRoot?.textContent).toContain("Final Content"));
    });

    it("renders preview_code source fallback when Rust snapshot says DOM is not yet paintable", async () => {
        const conversationId = 29;
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        const markdown =
            '<!-- MCP_TOOL_CALL_STREAMING:{"server_name":"ui_interaction","tool_name":"preview_code","fn_arguments":"{\\"title\\":\\"script_only\\",\\"renderer\\":\\"html\\",\\"code\\":\\"<script>console.log(\\\\\\"boot\\\\\\")</script>\\"}","llm_call_id":"preview_call_fallback","preview_state":{"title":"script_only","renderer":"html","code":"<script>console.log(\\"boot\\")</script>","loadingMessages":["正在生成交互面板"],"interactionMode":"none","hasRenderableDom":false,"containsScript":true,"renderableHtml":"","sourceExcerpt":"<script>console.log(\\"boot\\")</script>"}} -->';

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={20}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        expect(await screen.findByText("script_only")).toBeInTheDocument();
        expect(
            screen.getByText("正在生成可渲染预览，先展示当前代码片段。")
        ).toBeInTheDocument();
        expect(screen.getByText(/console\.log\("boot"\)/)).toBeInTheDocument();
    });

    it("merges streaming comment data when incomplete XML tag shadows it during dedup", async () => {
        const conversationId = 30;
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        // Simulate real streaming scenario: the model output has an incomplete
        // <mcp_tool_call> XML tag, and the backend appends a
        // <!-- MCP_TOOL_CALL_STREAMING --> comment AFTER it.
        // The XML extraction claims start=0, end=content.length (incomplete),
        // which would shadow the HTML comment. The dedup must merge the streaming
        // comment data into the XML entry.
        const previewState = {
            title: "steam_demo",
            renderer: "html",
            code: "<div>Steam Engine</div>",
            loadingMessages: ["正在生成交互面板"],
            interactionMode: "submit_once",
            hasRenderableDom: true,
            containsScript: false,
            renderableHtml: "<div>Steam Engine</div>",
            sourceExcerpt: "<div>Steam Engine</div>",
        };
        const markdown = [
            '<mcp_tool_call>',
            '<server_name>UI交互工具</server_name>',
            '<tool_name>preview_code</tool_name>',
            '<parameters>{"title":"steam_demo","renderer":"html","code":"<div>Steam Engine</div>","loading_messages":["正在生成交互面板"]',
            // ↑ Incomplete: no closing </parameters> or </mcp_tool_call>
            '',
            `<!-- MCP_TOOL_CALL_STREAMING:${JSON.stringify({
                server_name: "ui_interaction",
                tool_name: "preview_code",
                fn_arguments: '{"title":"steam_demo","renderer":"html","code":"<div>Steam Engine</div>","loading_messages":["正在生成交互面板"]}',
                llm_call_id: "call_merge_test",
                preview_state: previewState,
            })} -->`,
        ].join('\n');

        render(
            <ProcessorHarness
                markdown={markdown}
                conversationId={conversationId}
                messageId={21}
                mcpToolCallStates={new Map()}
                shiningMcpCallId={null}
            />
        );

        // The card should render with the streaming preview_state data
        expect(await screen.findByText("steam_demo")).toBeInTheDocument();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Steam Engine"),
        );
    });
});
