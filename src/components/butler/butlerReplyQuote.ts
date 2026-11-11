import type { Message } from "@/data/Conversation";
import { visibleText } from "@/utils/turnContext";

/**
 * 助手的引用回复：回复第一行写 `[[QUOTE:原话片段]]`，表示在回应包含这段原话的那条消息。
 * 模型看到的历史不带消息编号，所以按原话定位：从近到远找第一条包含该片段的消息，
 * 比较时忽略空白、标点、Markdown 符号与大小写。找不到时只去掉标记、不显示引用。
 *
 * 与 `src-tauri/src/utils/reply_quote.rs` 保持一致。
 */
const OPEN = "[[QUOTE:";
const CLOSE = "]]";
const MIN_FRAGMENT_CHARS = 2;
const MAX_FRAGMENT_CHARS = 200;

export interface ReplyQuoteParts {
    fragment: string;
    body: string;
}

/** 拆出回复开头的引用标记；没有标记、片段为空或过长、标记后没有正文时返回 null。 */
export function splitReplyQuote(reply: string): ReplyQuoteParts | null {
    const start = reply.trimStart();
    if (!start.startsWith(OPEN)) return null;
    const rest = start.slice(OPEN.length);
    const end = rest.indexOf(CLOSE);
    if (end < 0) return null;
    const fragment = rest.slice(0, end).trim();
    const body = rest.slice(end + CLOSE.length).replace(/^[ \t]+/, "").replace(/^[\r\n]+/, "");
    if (!fragment || fragment.includes("\n") || [...fragment].length > MAX_FRAGMENT_CHARS || !body.trim()) return null;
    return { fragment, body };
}

const normalize = (text: string) => text.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");

export function replyQuoteMatches(fragment: string, text: string): boolean {
    const needle = normalize(fragment);
    return [...needle].length >= MIN_FRAGMENT_CHARS && normalize(text).includes(needle);
}

// 用户消息只按原话匹配：本轮附加上下文（记忆、工作记录等）不是用户说过的话。
const isChatMessage = (message: Message) =>
    ["user", "response", "assistant"].includes(message.message_type) && visibleText(message.content).trim() !== "";

/** 在回复之前已显示的消息里，从近到远找出引用的那一条。 */
export function findQuotedMessage(fragment: string, earlier: Message[]): Message | undefined {
    for (let index = earlier.length - 1; index >= 0; index--) {
        const message = earlier[index];
        if (isChatMessage(message) && replyQuoteMatches(fragment, visibleText(message.content))) return message;
    }
    return undefined;
}
