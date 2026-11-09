import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ButlerGroupMessage } from "@/data/ButlerGroup";
import { AntiLeakageProvider } from "@/contexts/AntiLeakageContext";
import { ButlerQuoteContext, type ButlerQuoteTarget } from "./ButlerQuote";
import ButlerGroupMessageRow, { toChatMessage } from "./ButlerGroupMessageRow";

// 只替换渲染成本高的叶子组件；MessageItem、右键菜单、悬浮操作与引用标签用真实实现。
vi.mock("@/components/UnifiedMarkdown", () => ({
    default: ({ children, onCodeRun }: { children: string; onCodeRun?: (lang: string, code: string) => void }) => <div>
        {children}
        {onCodeRun && <button type="button" onClick={() => onCodeRun("html", "<b>hi</b>")}>运行代码</button>}
    </div>,
}));
vi.mock("@/components/token-statistics", () => ({
    MessageTokenStatsDialog: ({ messageId }: { messageId: number }) => <div data-testid="token-dialog">{messageId}</div>,
    MessageTokenTooltip: () => null,
}));

const base = { group_id: 7, sender_member: null, sender_assistant: null, sender_name: "我", kind: "message" as const,
    reply_to: null, created_at: "2026-11-09 08:00:00", source_conversation_id: null, source_message_id: null };
const memberReply: ButlerGroupMessage = { ...base, seq: 3, sender_member: 2, sender_assistant: 12, sender_name: "写作助手",
    body: "初稿如下\n\n```html\n<b>hi</b>\n```", source_conversation_id: 502, source_message_id: 9001 };
const userReply: ButlerGroupMessage = { ...base, seq: 4, body: "第二段改短", reply_to: 3 };

function renderRow(message: ButlerGroupMessage, quoted?: ButlerGroupMessage) {
    const setQuote = vi.fn((_: ButlerQuoteTarget | null) => {});
    // 管家窗口在外层提供防泄露上下文，这里同样提供。
    render(<AntiLeakageProvider enabled={false}><ButlerQuoteContext.Provider value={{ quote: null, setQuote }}>
        <ButlerGroupMessageRow groupId={7} message={message} quoted={quoted} />
    </ButlerQuoteContext.Provider></AntiLeakageProvider>);
    return setQuote;
}

describe("ButlerGroupMessageRow", () => {
    beforeEach(() => vi.mocked(invoke).mockClear());

    it("should point member replies at the real message in the member conversation", () => {
        const chat = toChatMessage(7, memberReply);
        expect(chat).toMatchObject({ id: 9001, conversation_id: 502, message_type: "response", content: memberReply.body });
        // 迁移前没有来源的旧消息不冒充会话消息，避免“消息信息”读到别的记录
        expect(toChatMessage(7, { ...memberReply, source_message_id: null, source_conversation_id: null }))
            .toMatchObject({ id: -3, conversation_id: 7, message_type: "assistant" });
        expect(toChatMessage(7, userReply, memberReply)).toMatchObject({
            id: -4, message_type: "user", metadata_json: JSON.stringify({ quote: { label: "写作助手：初稿如下 [代码]" } }),
        });
    });

    it("should open the same right-click menu as private chats on a member reply", async () => {
        const setQuote = renderRow(memberReply);
        fireEvent.contextMenu(screen.getByText(/初稿如下/), { clientX: 10, clientY: 10 });
        for (const label of ["复制消息", "消息信息", "导出消息"]) {
            expect(await screen.findByRole("menuitem", { name: label })).toBeInTheDocument();
        }
        // 引用回复与其他菜单项一致：图标 + 文字的菜单项，点击后收起菜单
        const quoteItem = await screen.findByRole("menuitem", { name: "引用回复" });
        expect(quoteItem).toHaveAttribute("data-aipp-slot", "butler-quote-reply");
        expect(quoteItem).toHaveTextContent("引用回复");
        expect(quoteItem.querySelector("svg")).not.toBeNull();
        fireEvent.click(quoteItem);
        expect(setQuote).toHaveBeenCalledWith({ kind: "message", id: 3, label: "写作助手", excerpt: "初稿如下 [代码]" });
        await waitFor(() => expect(document.querySelector('[data-aipp-slot="message-context-menu"]')).toBeNull());
    });

    it("should show token info of the source message from the menu", async () => {
        renderRow(memberReply);
        fireEvent.contextMenu(screen.getByText(/初稿如下/), { clientX: 10, clientY: 10 });
        fireEvent.click(await screen.findByRole("menuitem", { name: "消息信息" }));
        expect(await screen.findByTestId("token-dialog")).toHaveTextContent("9001");
    });

    it("should run code previews in the member conversation", async () => {
        renderRow(memberReply);
        fireEvent.click(screen.getByRole("button", { name: "运行代码" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("run_artifacts", { lang: "html", inputStr: "<b>hi</b>", conversationId: 502 }));
    });

    it("should show the member name above a full-width reply like private chats", () => {
        renderRow(memberReply);
        const row = document.querySelector('[data-aipp-slot="butler-group-member-message"]') as HTMLElement;
        const speaker = row.querySelector('[data-aipp-slot="butler-group-speaker"]') as HTMLElement;
        expect(speaker).toHaveTextContent("写作助手");
        // 发言人在上方单独一行，消息与私聊一样直接占整行，不再被左侧头像栏挤窄
        expect(row.firstElementChild).toBe(speaker);
        expect(row.querySelector(":scope > [data-message-item]")).not.toBeNull();
    });

    it("should label a quoted user message like private chats", () => {
        renderRow(userReply, memberReply);
        expect(document.querySelector('[data-aipp-slot="butler-message-quote"]')).toHaveTextContent("写作助手：初稿如下 [代码]");
        expect(screen.getByText("第二段改短")).toBeInTheDocument();
    });
});
