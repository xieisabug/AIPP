/** 管家长期记忆（后端 `db::butler_memory::Memory`）。 */
export interface ButlerMemory {
    id: number;
    content: string;
    /** assistant：仅该助手；group：本群成员共享；global：所有助手。 */
    scope_type: "assistant" | "group" | "global";
    scope_id: number | null;
    author_assistant_id: number | null;
    source_message_id: number | null;
    source_group_id: number | null;
    source_group_seq: number | null;
    revision: number;
    /** 毫秒时间戳。 */
    created_at: number;
    updated_at: number;
}

/** 记忆来源原话。deleted：原消息或会话已删除；none：没有记录来源。 */
export interface ButlerMemorySource {
    status: "available" | "deleted" | "none";
    label: string | null;
    text: string | null;
    truncated: boolean;
}

/** 单条记忆字数上限（与后端 `MAX_CONTENT_CHARS` 一致）。 */
export const BUTLER_MEMORY_MAX_CHARS = 300;

/**
 * 记忆字数：去掉首尾空白后按字符（Unicode 码点）计，与后端 `chars().count()` 一致。
 * 不能用 `string.length`（UTF-16 单元）：emoji 等会被算成两个字，和后端的上限对不上。
 */
export const butlerMemoryLength = (text: string) => Array.from(text.trim()).length;
