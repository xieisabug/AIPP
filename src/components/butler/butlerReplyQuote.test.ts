import { describe, expect, it } from "vitest";
import type { Message } from "@/data/Conversation";
import { findQuotedMessage, replyQuoteMatches, splitReplyQuote } from "./butlerReplyQuote";

const message = (id: number, message_type: string, content: string) => ({ id, message_type, content }) as Message;

// 与 src-tauri/src/utils/reply_quote_tests.rs 使用相同的用例，保证前后端判断一致。
describe("butlerReplyQuote", () => {
    it("should split a leading quote marker line from the reply body", () => {
        expect(splitReplyQuote("[[QUOTE:明天几点开会]]\n下午三点。")).toEqual({ fragment: "明天几点开会", body: "下午三点。" });
        expect(splitReplyQuote("  [[QUOTE: 预算 ]] 先按十万算。")).toEqual({ fragment: "预算", body: "先按十万算。" });
        expect(splitReplyQuote("[[QUOTE:方案 A]]\n\n```rust\n  let x = 1;\n```")?.body).toBe("```rust\n  let x = 1;\n```");
    });

    it("should ignore markers that are not at the start, empty, unterminated, too long or without a body", () => {
        expect(splitReplyQuote("下午三点。")).toBeNull();
        expect(splitReplyQuote("我觉得 [[QUOTE:预算]] 可以")).toBeNull();
        expect(splitReplyQuote("[[QUOTE:]]\n正文")).toBeNull();
        expect(splitReplyQuote("[[QUOTE:预算]]\n  ")).toBeNull();
        expect(splitReplyQuote("[[QUOTE:预算\n还没结束")).toBeNull();
        expect(splitReplyQuote(`[[QUOTE:${"长".repeat(201)}]]\n正文`)).toBeNull();
    });

    it("should match fragments regardless of punctuation, markdown and case", () => {
        const text = "我们下周要不要把 **API 网关** 换成自建的？预算大概十万。";
        expect(replyQuoteMatches("API网关换成自建的", text)).toBe(true);
        expect(replyQuoteMatches("「api 网关」换成自建的?", text)).toBe(true);
        expect(replyQuoteMatches("预算大概二十万", text)).toBe(false);
        expect(replyQuoteMatches("？", text)).toBe(false);
        expect(replyQuoteMatches("预", text)).toBe(false);
    });

    it("should pick the most recent chat message containing the fragment", () => {
        const earlier = [message(1, "user", "评审几点开始"), message(2, "tool_result", "评审几点开始"), message(3, "user", "评审几点开始？我也想知道")];
        expect(findQuotedMessage("评审几点开始", earlier)?.id).toBe(3);
        expect(findQuotedMessage("评审几点开始", earlier.slice(0, 2))?.id).toBe(1);
        expect(findQuotedMessage("没人说过", earlier)).toBeUndefined();
    });
});
