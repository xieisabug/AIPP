import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Message } from "@/data/Conversation";
import {
    ButlerMessageQuoteLabel, ButlerQuoteButton, ButlerQuoteChip, ButlerQuoteContext, ButlerQuoteJumpContext,
    isQuotableMessage, quoteExcerpt, readMessageQuote,
} from "./ButlerQuote";

const message = (extra: Partial<Message>) => ({ id: 5, message_type: "response", content: "结果", finish_time: new Date(), ...extra }) as Message;

describe("Butler quote reply", () => {
    it("should only offer quoting on finished assistant replies", () => {
        expect(isQuotableMessage(message({}))).toBe(true);
        expect(isQuotableMessage(message({ finish_time: null }))).toBe(false);
        expect(isQuotableMessage(message({ message_type: "user" }))).toBe(false);
        expect(isQuotableMessage(message({ id: -3 }))).toBe(false);
    });

    it("should read the persisted quote identity after reopening the conversation", () => {
        const quoted = message({ message_type: "user", metadata_json: JSON.stringify({ quote: { ref: "task:41", label: "任务《周报》" } }) });
        expect(readMessageQuote(quoted)).toEqual({ label: "任务《周报》" });
        const lost = message({ message_type: "user", metadata_json: JSON.stringify({ quote: { unavailable: "已删除" } }) });
        expect(readMessageQuote(lost)?.unavailable).toBe("已删除");
        expect(readMessageQuote(message({ message_type: "user", metadata_json: "{bad" }))).toBeNull();
        render(<ButlerMessageQuoteLabel message={quoted} />);
        expect(screen.getByText("任务《周报》")).toBeVisible();
    });

    it("should label an assistant reply's quote on its own side and jump to the quoted message", () => {
        const reply = message({ reply_quote: { label: "我：明天几点开会", target: 3 } });
        expect(readMessageQuote(reply)).toEqual({ label: "我：明天几点开会", target: 3 });
        const jump = vi.fn();
        render(<ButlerQuoteJumpContext.Provider value={jump}><ButlerMessageQuoteLabel message={reply} /></ButlerQuoteJumpContext.Provider>);
        const label = screen.getByTitle("查看被引用的消息");
        expect(label.closest('[data-aipp-slot="butler-message-quote"]')?.className).toContain("justify-start");
        fireEvent.click(label);
        expect(jump).toHaveBeenCalledWith(3);
        // 用户引用的是本会话的某条回复时同样可以跳过去；没有可跳转的目标时只显示文字
        const quotedReply = message({ message_type: "user", metadata_json: JSON.stringify({ quote: { kind: "message", id: 9, label: "引用回复" } }) });
        expect(readMessageQuote(quotedReply)).toEqual({ label: "引用回复", target: 9 });
        expect(readMessageQuote(message({}))).toBeNull();
    });

    it("should set the quote from a reply and clear it from the input chip", () => {
        const setQuote = vi.fn();
        const target = { kind: "message" as const, id: 5, label: "引用回复", excerpt: quoteExcerpt("## 标题\n正文") };
        render(<ButlerQuoteContext.Provider value={{ quote: null, setQuote }}><ButlerQuoteButton target={target} /></ButlerQuoteContext.Provider>);
        fireEvent.click(screen.getByRole("button", { name: "引用回复" }));
        expect(setQuote).toHaveBeenCalledWith({ ...target, excerpt: "标题 正文" });
        const onClear = vi.fn();
        render(<ButlerQuoteChip quote={target} onClear={onClear} />);
        fireEvent.click(screen.getByRole("button", { name: "取消引用" }));
        expect(onClear).toHaveBeenCalled();
    });

    it("should not render a quote action outside Butler chats", () => {
        const view = render(<ButlerQuoteButton target={{ kind: "task", id: 1, label: "x", excerpt: "" }} />);
        expect(view.container).toBeEmptyDOMElement();
    });
});
