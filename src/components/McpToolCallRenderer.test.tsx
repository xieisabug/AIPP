import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import McpToolCallRenderer from "./McpToolCallRenderer";
import { ButlerResultsOnlyContext } from "./butler/ButlerPresentationContext";
import { noteToolReview, resetToolReviewsForTests } from "@/hooks/toolReviewStore";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/hooks/useDisplayConfig", () => ({ useDisplayConfig: () => ({ config: {} }) }));
vi.mock("@/services/builtinMcpToolComponents", () => ({ ensureBuiltinMcpToolComponentsRegistered: () => {} }));
vi.mock("@/services/mcpToolComponentRegistry", () => ({
    AUTO_MCP_TOOL_COMPONENT_ID: "auto",
    useMcpToolComponentRegistrySnapshot: () => {},
    mcpToolComponentRegistry: {
        resolve: () => ({ id: "custom", render: () => <div>专用工具卡片</div> }),
    },
}));
vi.mock("@/components/McpToolCall", () => ({ default: () => <div>完整审核操作卡片</div> }));

describe("McpToolCallRenderer review controls", () => {
    afterEach(() => {
        clearAllMockHandlers();
        resetToolReviewsForTests();
    });

    it("should never mount an execution card in compact Butler mode across tool states", () => {
        const draw = (status: "pending" | "executing" | "failed" | "success", resultsOnly = true) =>
            <ButlerResultsOnlyContext.Provider value={resultsOnly}>
                <McpToolCallRenderer callId={94} conversationId={8} toolName="spawn_task_conversation" status={status}/>
            </ButlerResultsOnlyContext.Provider>;
        const view = render(draw("pending"));
        for (const status of ["pending","executing","failed","success"] as const) {
            view.rerender(draw(status));
            expect(view.container).toBeEmptyDOMElement();
        }
        view.rerender(draw("success", false));
        expect(screen.getByText("专用工具卡片")).toBeInTheDocument();
    });

    it.each(["preview_file", "ask_user_question", "preview_code"])("should show only the result for %s in compact chat", (name) => {
        mockInvokeHandler("get_mcp_tool_call", () => ({ id: 95, status: "executing", tool_name: name, conversation_id: 8 }));
        render(<ButlerResultsOnlyContext.Provider value={true}>
            <McpToolCallRenderer callId={95} conversationId={8} toolName={name} status="executing" />
        </ButlerResultsOnlyContext.Provider>);
        expect(screen.queryByText("专用工具卡片")).toBeNull();
        expect(screen.queryByText("完整审核操作卡片")).toBeNull();
    });

    it("should restore the file content without a tool invocation frame", async () => {
        const files = [{ title: "周报.txt", type: "text", content: "本周交付了文件预览" }];
        mockInvokeHandler("get_mcp_tool_call", () => ({ id: 105, conversation_id: 8, status: "success",
            tool_name: "preview_file", parameters: JSON.stringify({ files }) }));
        mockInvokeHandler("prepare_preview_file_request_for_ui", args => args?.request);
        render(<ButlerResultsOnlyContext.Provider value={true}>
            <McpToolCallRenderer callId={105} conversationId={8} toolName="preview_file" status="success" />
        </ButlerResultsOnlyContext.Provider>);
        expect(await screen.findByText("周报.txt")).toBeInTheDocument();
        expect(screen.getByText("本周交付了文件预览")).toBeInTheDocument();
        expect(screen.queryByText("完整审核操作卡片")).toBeNull();
        expect(screen.queryByText("专用工具卡片")).toBeNull();
    });

    it("should keep the preview_message card in compact Butler mode because it is the delivered result", () => {
        render(<ButlerResultsOnlyContext.Provider value={true}>
            <McpToolCallRenderer callId={96} conversationId={8} toolName="preview_message" status="success"/>
        </ButlerResultsOnlyContext.Provider>);
        expect(screen.getByText("专用工具卡片")).toBeInTheDocument();
    });

    it("should use confirmation controls when a specialized tool receives a live review", async () => {
        mockInvokeHandler("get_tool_review", () => null);
        render(<McpToolCallRenderer callId={91} conversationId={8} status="pending" toolName="fetch_url" />);
        expect(screen.getByText("专用工具卡片")).toBeInTheDocument();
        await act(async () => { noteToolReview(91, { phase: "reviewing", callId: 91 }); });
        expect(screen.getByText("完整审核操作卡片")).toBeInTheDocument();
        await act(async () => {
            noteToolReview(91, { phase: "done", callId: 91, verdict: "error", reason: "格式错误" });
        });
        expect(screen.getByText("完整审核操作卡片")).toBeInTheDocument();
        expect(screen.queryByText("专用工具卡片")).not.toBeInTheDocument();
    });

    it("should load historical review controls when the call id is resolved through llm_call_id", async () => {
        mockInvokeHandler("get_tool_review", () => ({ verdict: "risky", reason: "目标不明" }));
        render(<McpToolCallRenderer conversationId={8} llmCallId="call-92" toolName="search_web"
            mcpToolCallStates={new Map([[92, {
                call_id: 92, conversation_id: 8, status: "pending", llm_call_id: "call-92",
                server_name: "builtin", tool_name: "search_web", parameters: "{}",
            }]])} />);
        expect(await screen.findByText("完整审核操作卡片")).toBeInTheDocument();
    });

    it("should preserve the specialized result when a reviewed call has completed", () => {
        noteToolReview(93, { phase: "done", callId: 93, verdict: "safe", reason: "只读" });
        render(<McpToolCallRenderer callId={93} conversationId={8} status="success" toolName="search_web" />);
        expect(screen.getByText("专用工具卡片")).toBeInTheDocument();
        expect(screen.queryByText("完整审核操作卡片")).not.toBeInTheDocument();
    });
});
