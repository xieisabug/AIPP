// 前端复刻自后端 Rust chat.rs 中的 @assistant 解析逻辑
// 仅实现当前需要的默认行为：匹配第一个 @助手名 (大小写不敏感, 词边界)，并从内容中移除它。
// 若后续需要支持多 @ 或保留文本，可扩展 ParseOptions。

import { AssistantListItem } from "../data/Assistant";

export interface AssistantMention {
    assistant_id: number;
    name: string;
    start_pos: number; // 字符位置
    end_pos: number;   // 结束位置 (开区间)
    raw_match: string; // 原始匹配字符串 "@name"
}

export interface MessageParseResult {
    mentions: AssistantMention[];
    cleaned_content: string;          // 移除 mentions 后的内容
    original_content: string;         // 原始内容
    primary_assistant_id?: number;    // 第一个匹配的助手 id
}

export type PositionRestriction = "anywhere" | "start" | "word-boundary";

export interface ParseOptions {
    first_only: boolean;
    position_restriction: PositionRestriction;
    remove_mentions: boolean;
    case_sensitive: boolean;
    require_word_boundary: boolean;
}

export const defaultParseOptions: ParseOptions = {
    first_only: true,
    position_restriction: "anywhere",
    remove_mentions: true,
    case_sensitive: false,
    require_word_boundary: true,
};

// 词边界结束：若结束位置在文本末尾，或下一个字符不是字母/数字/下划线/连字符
function isWordBoundaryEnd(chars: string[], pos: number): boolean {
    if (pos >= chars.length) return true;
    const next = chars[pos];
    return !(/[0-9A-Za-z_-]/.test(next));
}

function checkPositionRestriction(chars: string[], pos: number, r: PositionRestriction): boolean {
    switch (r) {
        case "anywhere":
            return true;
        case "start":
            return pos === 0;
        case "word-boundary":
            return pos === 0 || chars[pos - 1] === ' ';
        default:
            return true;
    }
}

function tryMatchSpecificAssistant(
    assistant: AssistantListItem,
    chars: string[],
    start_pos: number,
    options: ParseOptions,
): AssistantMention | undefined {
    if (chars[start_pos] !== '@') return;

    const pattern = `@${assistant.name}`;
    // 长度不足
    if (start_pos + pattern.length > chars.length) return;

    const slice = chars.slice(start_pos, start_pos + pattern.length).join('');
    const matched = options.case_sensitive
        ? slice === pattern
        : slice.toLowerCase() === pattern.toLowerCase();
    if (!matched) return;

    const end_pos = start_pos + pattern.length;
    if (options.require_word_boundary && !isWordBoundaryEnd(chars, end_pos)) return;
    if (!options.require_word_boundary && end_pos < chars.length) {
        const next = chars[end_pos];
        if (/[0-9A-Za-z]/.test(next)) return; // 避免 @gpt4help 匹配 @gpt4
    }

    return {
        assistant_id: assistant.id,
        name: assistant.name,
        start_pos,
        end_pos,
        raw_match: pattern,
    };
}

function tryMatchAssistantAtPosition(
    assistants: AssistantListItem[],
    chars: string[],
    start_pos: number,
    options: ParseOptions,
): AssistantMention | undefined {
    if (chars[start_pos] !== '@') return;
    if (!checkPositionRestriction(chars, start_pos, options.position_restriction)) return;

    // 名称按长度从长到短，避免前缀部分匹配
    const sorted = [...assistants].sort((a, b) => b.name.length - a.name.length);
    for (const a of sorted) {
        const m = tryMatchSpecificAssistant(a, chars, start_pos, options);
        if (m) return m;
    }
    return;
}

function removeMentionsFromContent(content: string, mentions: AssistantMention[]): string {
    if (!mentions.length) return content;
    const chars = [...content];
    const sorted = [...mentions].sort((a, b) => a.start_pos - b.start_pos);
    const result: string[] = [];
    let i = 0;
    for (const mention of sorted) {
        while (i < mention.start_pos) {
            result.push(chars[i]);
            i++;
        }
        // 跳过 mention
        i = mention.end_pos;
        if (i < chars.length) {
            const next = chars[i];
            if (",.!?;:，。！？；：".includes(next)) {
                i++; // 跳过标点
                while (i < chars.length && /\s/.test(chars[i])) i++; // 跳过后续空格
            } else if (/\s/.test(next)) {
                while (i < chars.length && /\s/.test(chars[i])) i++;
            }
        }
    }
    while (i < chars.length) {
        result.push(chars[i]);
        i++;
    }
    const merged = result.join('');
    return merged.split(/\s+/).join(' ').trim();
}

export function parseAssistantMentions(
    assistants: AssistantListItem[],
    content: string,
    options: Partial<ParseOptions> = {},
): MessageParseResult {
    const opt: ParseOptions = { ...defaultParseOptions, ...options };
    const chars = [...content];
    const mentions: AssistantMention[] = [];
    let i = 0;
    while (i < chars.length) {
        if (chars[i] === '@') {
            const m = tryMatchAssistantAtPosition(assistants, chars, i, opt);
            if (m) {
                mentions.push(m);
                i = m.end_pos; // 跳到 mention 末尾
                if (opt.first_only) break;
                continue;
            }
        }
        i++;
    }

    const cleaned_content = opt.remove_mentions
        ? removeMentionsFromContent(content, mentions)
        : content;

    return {
        mentions,
        cleaned_content,
        original_content: content,
        primary_assistant_id: mentions[0]?.assistant_id,
    };
}

// 前端暴露的与后端同名的函数，用于在发送前覆盖 assistantId 和 prompt
export function extractAssistantFromMessage(
    assistants: AssistantListItem[],
    prompt: string,
    defaultAssistantId: number,
): { assistantId: number; cleanedPrompt: string } {
    const result = parseAssistantMentions(assistants, prompt, {});
    return {
        assistantId: result.primary_assistant_id ?? defaultAssistantId,
        cleanedPrompt: result.cleaned_content,
    };
}
