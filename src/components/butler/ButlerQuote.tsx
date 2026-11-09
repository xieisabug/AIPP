import { createContext, useContext } from "react";
import { Quote, X } from "lucide-react";
import type { Message } from "@/data/Conversation";
import IconButton from "@/components/IconButton";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { useMessageActionSurface } from "@/components/message-item/messageActionSurface";

/** 聊天式引用回复的对象：后台按 kind + id（稳定身份）关联原工作，不靠文字或时间猜测。 */
export interface ButlerQuoteTarget {
    kind: "message" | "task" | "exchange";
    id: number;
    label: string;
    excerpt: string;
}

export interface ButlerQuoteState {
    quote: ButlerQuoteTarget | null;
    setQuote: (quote: ButlerQuoteTarget | null) => void;
}

/** 只在管家联系人/总管家会话中提供；普通聊天为 null，不出现引用入口。 */
export const ButlerQuoteContext = createContext<ButlerQuoteState | null>(null);

export const useButlerQuote = () => useContext(ButlerQuoteContext);

/**
 * 点击引用标签时跳到被引用的那条消息。参数是标签上的 target：私聊里是消息 id，群聊里是群消息序号，
 * 由提供方各自解释。没有提供时标签只显示文字、不可点击。
 */
export const ButlerQuoteJumpContext = createContext<((target: number) => void) | null>(null);

export function quoteExcerpt(text: string, limit = 60): string {
    const plain = text
        .replace(/<!--[\s\S]*?-->/g, "")
        .replace(/```[\s\S]*?```/g, "[代码]")
        .replace(/[#>*_`]/g, "")
        .replace(/\s+/g, " ")
        .trim();
    return plain.length > limit ? `${plain.slice(0, limit)}…` : plain;
}

/** 已完成、有正文的助手回复可以被引用。 */
export function isQuotableMessage(message: Message): boolean {
    return (message.message_type === "response" || message.message_type === "assistant")
        && message.id > 0 && message.finish_time !== null && message.content.trim() !== "";
}

export interface MessageQuoteLabel {
    label: string;
    unavailable?: string;
    /** 被引用消息在当前列表里的 id，可跳转。 */
    target?: number;
}

/**
 * 消息上显示的引用：用户消息读发送时写入 metadata 的引用身份（重进或重启后仍可显示）；
 * 助手回复读显示时从开头 `[[QUOTE:…]]` 解析出的 `reply_quote`。
 */
export function readMessageQuote(message: Message): MessageQuoteLabel | null {
    if (message.message_type !== "user") return message.reply_quote ?? null;
    if (!message.metadata_json) return null;
    try {
        const quote = JSON.parse(message.metadata_json)?.quote;
        if (!quote) return null;
        if (quote.unavailable) return { label: "引用的记录不可用", unavailable: String(quote.unavailable) };
        if (typeof quote.label !== "string") return null;
        return quote.kind === "message" && typeof quote.id === "number" ? { label: quote.label, target: quote.id } : { label: quote.label };
    } catch {
        return null;
    }
}

/**
 * `toolbar`：放在消息悬浮操作条里，与复制/重新生成等按钮同尺寸同颜色（IconButton + text-icon）。
 * `inline`：放在小号文字行（如直接交付行的标题）里，跟随行内文字颜色。
 */
export function ButlerQuoteButton({ target, variant = "inline" }: { target: ButlerQuoteTarget; variant?: "inline" | "toolbar" }) {
    const state = useButlerQuote();
    const surface = useMessageActionSurface();
    if (!state) return null;
    // 右键菜单里和其他菜单项一致：图标 + 文字，占满一行。
    if (surface === "menu") {
        return <DropdownMenuItem className="basis-full" data-aipp-slot="butler-quote-reply" onSelect={() => state.setQuote(target)}>
            <Quote className="h-4 w-4" />引用回复
        </DropdownMenuItem>;
    }
    if (variant === "toolbar") {
        return <IconButton icon={<Quote size={16} className="text-icon" />} title="引用回复"
            dataAippSlot="butler-quote-reply" onClick={() => state.setQuote(target)} />;
    }
    return <button type="button" title="引用回复" aria-label="引用回复" data-aipp-slot="butler-quote-reply"
        onClick={() => state.setQuote(target)}
        className="inline-flex size-6 items-center justify-center rounded text-muted-foreground hover:bg-muted hover:text-foreground">
        <Quote className="h-3.5 w-3.5" />
    </button>;
}

export function ButlerQuoteChip({ quote, onClear }: { quote: ButlerQuoteTarget; onClear: () => void }) {
    return <div className="flex min-w-0 max-w-full items-center gap-1.5 rounded-md bg-muted px-2 py-0.5 text-xs text-muted-foreground"
        data-aipp-slot="butler-quote-chip">
        <Quote className="h-3 w-3 shrink-0" />
        <span className="shrink-0 font-medium text-foreground">{quote.label}</span>
        {quote.excerpt && <span className="min-w-0 truncate">{quote.excerpt}</span>}
        <button type="button" aria-label="取消引用" onClick={onClear} className="shrink-0 rounded hover:text-foreground">
            <X className="h-3 w-3" />
        </button>
    </div>;
}

export function ButlerMessageQuoteLabel({ message }: { message: Message }) {
    const jump = useContext(ButlerQuoteJumpContext);
    const quote = readMessageQuote(message);
    if (!quote) return null;
    const align = message.message_type === "user" ? "justify-end" : "justify-start";
    const content = <>
        <Quote className="h-3 w-3 shrink-0" />
        <span className="truncate">{quote.label}</span>
    </>;
    const target = quote.target;
    return <div className={`mb-1 flex min-w-0 ${align} text-xs text-muted-foreground`} data-aipp-slot="butler-message-quote"
        title={quote.unavailable}>
        {target !== undefined && jump
            ? <button type="button" title="查看被引用的消息" onClick={() => jump(target)}
                className="inline-flex min-w-0 items-center gap-1 rounded hover:text-foreground">{content}</button>
            : <span className="inline-flex min-w-0 items-center gap-1">{content}</span>}
    </div>;
}
