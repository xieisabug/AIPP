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

    it("should never mount preview or custom tool renderers when compact even before status is available", () => {
        const draw = (status?: React.ComponentProps<typeof McpToolCallRenderer>["status"], streaming = false) =>
            <ButlerResultsOnlyContext.Provider value={true}>
                <McpToolCallRenderer callId={95} conversationId={8} toolName="preview_code" status={status} isStreaming={streaming}/>
            </ButlerResultsOnlyContext.Provider>;
        const view = render(draw());
        for (const status of [undefined,"unknown","pending","executing","success","failed"] as const) {
            view.rerender(draw(status, true));
            expect(view.container).toBeEmptyDOMElement();
            view.rerender(draw(status));
            expect(view.container).toBeEmptyDOMElement();
        }
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
