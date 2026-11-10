/**
 * 本轮附加上下文：系统随用户消息附带给模型的当前时间、工作记录、引用的工作、长期记忆等。
 * 它用 `<aipp_turn_context>` 包住、放在用户消息正文最末尾，和原话一起存储，模型历史逐字回放。
 * 界面只显示用户原话；附加上下文（包括其中的长期记忆）只给模型看，界面不显示。
 * 只有以 `</aipp_turn_context>` 结尾的正文才算带附加上下文，用户自己打出这个标签不会被误判。
 *
 * 与 `src-tauri/src/utils/turn_context.rs` 保持一致。
 */
export const TURN_CONTEXT_OPEN = "<aipp_turn_context>";
export const TURN_CONTEXT_CLOSE = "</aipp_turn_context>";
const HEADER = "以下是系统随这条消息附带的上下文，不是用户写的内容。";

export interface TurnContextParts {
    /** 用户原话（含附件标签）。 */
    text: string;
    /** 附加上下文正文（不含外层标签与说明行）；没有时为 null。 */
    context: string | null;
    /** 完整的附加上下文块，编辑原话后原样接回去；没有时为 null。 */
    block: string | null;
}

export function splitTurnContext(content: string): TurnContextParts {
    const trimmed = content.trimEnd();
    const at = trimmed.endsWith(TURN_CONTEXT_CLOSE) ? trimmed.indexOf(TURN_CONTEXT_OPEN) : -1;
    if (at < 0) return { text: content, context: null, block: null };
    let inner = trimmed.slice(at + TURN_CONTEXT_OPEN.length, trimmed.length - TURN_CONTEXT_CLOSE.length).trim();
    if (inner.startsWith(HEADER)) inner = inner.slice(HEADER.length).trim();
    return { text: trimmed.slice(0, at).trimEnd(), context: inner, block: trimmed.slice(at) };
}

/** 给人看的部分：用户消息去掉末尾的附加上下文，其他消息原样返回。 */
export function visibleText(content: string): string {
    return splitTurnContext(content).text;
}

/** 编辑原话后把原来的附加上下文接回末尾（与后端 `turn_context::append` 一致）。 */
export function withTurnContext(text: string, block: string | null): string {
    if (!block) return text;
    if (!text.trim()) return block;
    return `${text.trimEnd()}\n\n${block}`;
}
