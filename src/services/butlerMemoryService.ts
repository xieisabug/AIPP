import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ButlerMemory, ButlerMemorySource } from "@/data/ButlerMemory";

export const BUTLER_MEMORY_CHANGED_EVENT = "butler_memory_changed";

/** 会话能看到的记忆：私聊/总管家为“该助手 + 全局”，群聊为“本群 + 全局”。 */
export const listButlerMemories = (conversationId: number) =>
    invoke<ButlerMemory[]>("list_butler_memories", { conversationId });

/** 设置页按助手或总管家身份读取记忆（传 null/undefined 表示总管家 + 全局）。 */
export const listAssistantButlerMemories = (assistantId?: number | null) =>
    invoke<ButlerMemory[]>("list_assistant_butler_memories", {
        assistantId: assistantId ?? null,
    });

/** 带上读取时的 revision，别处刚改过时会报错而不是覆盖。 */
export const updateButlerMemory = (id: number, content: string, revision: number) =>
    invoke<ButlerMemory>("update_butler_memory", { id, content, revision });

export const deleteButlerMemory = (id: number, revision: number) =>
    invoke<boolean>("delete_butler_memory", { id, revision });

export const getButlerMemorySource = (id: number) =>
    invoke<ButlerMemorySource>("get_butler_memory_source", { id });

/** 助手在聊天里或用户在界面里改动记忆后回调。 */
export const onButlerMemoryChanged = (callback: () => void) =>
    listen(BUTLER_MEMORY_CHANGED_EVENT, () => callback());
