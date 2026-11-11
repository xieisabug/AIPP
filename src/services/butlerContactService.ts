import { invoke } from "@tauri-apps/api/core";

export interface ButlerContactResetResult {
    /** 新开的私聊。 */
    conversationId: number;
    /** 被归档的旧私聊；之前从未打开过私聊时为空。 */
    archivedConversationIds: number[];
    /** 停止旧私聊上进行中工作的说明；没有进行中的工作时为空。 */
    stopNotes: string[];
}

/**
 * 重开助手在管家中的私聊（与总管家「重开新会话」一致）：旧私聊归档为只读存档，
 * 仍可在管家搜索和助手历史检索中找到；属于旧私聊的进行中工作由后台停止。不影响普通聊天、任务与长期记忆。
 * 失败时原样抛出后台给出的环节与原因（含助手、会话 ID），由调用方加上操作名展示。
 */
export function resetButlerContactConversation(assistantId: number): Promise<ButlerContactResetResult> {
    return invoke<ButlerContactResetResult>("reset_butler_contact_conversation", { assistantId });
}
