/**
 * 本轮附加上下文：系统随用户消息附带给模型的当前时间、工作记录、引用的工作、长期记忆等。
 * 它用 `<aipp_turn_context>` 包住、放在用户消息正文最末尾，和原话一起存储，模型历史逐字回放。
 * 界面只显示用户原话；附加上下文（包括其中的长期记忆）只给模型看，界面不显示。
 * 识别规则：正文以 `</aipp_turn_context>` 结尾，且含有系统写出的块开头（开标签紧跟说明行），从第一处这样的开头算起。
 * 用户自己打出 `<aipp_turn_context>…</aipp_turn_context>` 不带说明行，不会把后面的原话当成附加上下文藏掉。
 *
 * 与 `src-tauri/src/utils/turn_context.rs` 保持一致。
 */
const TURN_CONTEXT_CLOSE = "</aipp_turn_context>";
/** 块开头：开标签 + 换行 + 说明行，只由后端 `turn_context::wrap` 写出。 */
const TURN_CONTEXT_START = "<aipp_turn_context>\n以下是系统随这条消息附带的上下文，不是用户写的内容。";

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
    if (!trimmed.endsWith(TURN_CONTEXT_CLOSE)) return { text: content, context: null, block: null };
    const body = trimmed.slice(0, trimmed.length - TURN_CONTEXT_CLOSE.length);
    const at = body.indexOf(TURN_CONTEXT_START);
    if (at < 0) return { text: content, context: null, block: null };
    return {
        text: trimmed.slice(0, at).trimEnd(),
        context: body.slice(at + TURN_CONTEXT_START.length).trim(),
        block: trimmed.slice(at),
    };
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
