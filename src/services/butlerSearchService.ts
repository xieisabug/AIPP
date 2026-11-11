import { invoke } from "@tauri-apps/api/core";
import type { ButlerMessageSearchHit } from "../data/Conversation";

interface RawButlerSearchHit {
    message_id: number;
    conversation_id: number;
    conversation_kind: ButlerMessageSearchHit["conversation_kind"];
    conversation_name: string;
    bot_id: number | null;
    bot_name: string;
    role: string;
    speaker: string;
    created_time: string;
    snippet: string;
}

/** 搜索结果分组标题：私聊和派工会话本身的名字不足以说明来源，补上类型；归档会话的名字已带“存档”时间。 */
export function butlerGroupName(kind: RawButlerSearchHit["conversation_kind"], name: string, mainConversationId?: number, conversationId?: number) {
    if (kind === "butler_contact") return "私聊";
    if (kind === "butler_contact_archive") return name;
    if (kind === "butler_task") return `派工任务「${name}」`;
    return conversationId === mainConversationId ? "当前主会话" : name;
}

export async function searchButlerMessages(
    query: string,
    limit: number = 50,
    offset: number = 0,
    mainConversationId?: number,
): Promise<ButlerMessageSearchHit[]> {
    const trimmed = query.trim();
    if (!trimmed) return [];
    const rows = await invoke<RawButlerSearchHit[]>("search_butler_messages", { query: trimmed, limit, offset });
    return rows.map(row => ({
        conversation_id: row.conversation_id,
        conversation_name: butlerGroupName(row.conversation_kind, row.conversation_name, mainConversationId, row.conversation_id),
        assistant_name: row.bot_name,
        message_id: row.message_id,
        message_type: row.role,
        // SQLite 时间形如 "2026-11-07 10:00:00.123+00:00"，转成 ISO 形式保证各引擎都能解析
        created_time: new Date(row.created_time.replace(" ", "T")),
        snippet: row.snippet,
        hit_type: "message",
        speaker: row.speaker,
        conversation_kind: row.conversation_kind,
        assistant_id: row.bot_id,
    }));
}
