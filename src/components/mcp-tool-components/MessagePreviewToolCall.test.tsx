import { act, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import MessagePreviewToolCall, {
    resolvePreviewReference,
    type MessagePreviewData,
} from "@/components/mcp-tool-components/MessagePreviewToolCall";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/components/UnifiedMarkdown", () => ({
    default: ({ children }: { children: string }) => <div data-testid="markdown">{children}</div>,
}));

const flushEffects = async () => {
    await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
    });
};

const receipt = (messageId: number) => JSON.stringify({
    content: [{ type: "text", text: JSON.stringify({ status: "preview_shown", message_id: messageId, author: "写作" }) }],
    isError: false,
});

const preview = (content = "周报初稿：\n- 完成 A"): MessagePreviewData => ({
    message_id: 42,
    conversation_id: 7,
    conversation_kind: "butler_contact",
    source: "与写作的私聊",
    author: "写作",
    created_time: "2026-11-12T08:00:00Z",
    archived: false,
    content,
});

const props = (overrides: Partial<McpToolComponentProps> = {}): McpToolComponentProps => ({
    serverName: "Agent 工具",
    toolName: "preview_message",
    parameters: '{"message_id":42}',
    status: "success",
    conversationId: 3,
    messageId: 9,
    ...overrides,
});

describe("resolvePreviewReference", () => {
    it("should prefer the message id resolved in the receipt over the original work_ref", () => {
        expect(resolvePreviewReference('{"work_ref":"exchange:5","title":"初稿"}', receipt(77)))
            .toEqual({ messageId: 77, title: "初稿" });
    });

    it("should fall back to call parameters before the tool finishes", () => {
        expect(resolvePreviewReference('{"message_id":"#12"}')).toEqual({ messageId: 12, workRef: undefined, title: undefined });
        expect(resolvePreviewReference('{"work_ref":" task:3 "}')).toEqual({ messageId: undefined, workRef: "task:3", title: undefined });
        expect(resolvePreviewReference('{"message_id":0}')).toBeNull();
        expect(resolvePreviewReference('{"message_id":')).toBeNull();
    });
});

describe("MessagePreviewToolCall", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
    });

    it("should show the referenced reply with its author and source when the tool succeeded", async () => {
        mockInvokeHandler("get_message_preview", () => preview());
        render(<MessagePreviewToolCall {...props({ parameters: '{"work_ref":"exchange:5","title":"写作的初稿"}', currentToolCall: {
            call_id: 1, conversation_id: 3, status: "success", result: receipt(42),
        } })} />);
        await flushEffects();

        expect(invoke).toHaveBeenCalledWith("get_message_preview", { viewerConversationId: 3, messageId: 42, workRef: null });
        expect(screen.getByText("写作的初稿")).toBeInTheDocument();
        expect(screen.getByText("与写作的私聊")).toBeInTheDocument();
        expect(screen.getByTestId("markdown")).toHaveTextContent("周报初稿：");
        expect(screen.queryByText("展开全文")).not.toBeInTheDocument();
    });

    it("should not read the message while the call is still running", async () => {
        render(<MessagePreviewToolCall {...props({ status: "executing" })} />);
        await flushEffects();
        expect(invoke).not.toHaveBeenCalled();
        expect(screen.getByText("正在展示引用的消息…")).toBeInTheDocument();
    });

    it("should explain why a failed or no longer readable reference cannot be shown", async () => {
        const { unmount } = render(<MessagePreviewToolCall {...props({ status: "failed", error: "消息 #42 已删除或不存在" })} />);
        await flushEffects();
        expect(invoke).not.toHaveBeenCalled();
        expect(screen.getByText(/引用的消息无法展示：消息 #42 已删除或不存在/)).toBeInTheDocument();
        unmount();

        mockInvokeHandler("get_message_preview", () => { throw new Error("消息 #42 不在管家会话或管家定时任务里，不能展示"); });
        render(<MessagePreviewToolCall {...props()} />);
        await flushEffects();
        expect(screen.getByText(/不在管家会话或管家定时任务里/)).toBeInTheDocument();
    });

    it("should collapse a long reply until the user expands it", async () => {
        mockInvokeHandler("get_message_preview", () => preview("段落\n".repeat(40)));
        render(<MessagePreviewToolCall {...props()} />);
        await flushEffects();

        await userEvent.click(screen.getByRole("button", { name: "展开全文" }));
        expect(screen.getByRole("button", { name: "收起" })).toBeInTheDocument();
    });
});
