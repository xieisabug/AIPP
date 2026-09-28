import { act, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import McpToolCallRenderer from "./McpToolCallRenderer";
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
