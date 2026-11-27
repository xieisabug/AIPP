import { invoke } from "@tauri-apps/api/core";
import { emitTo, once } from "@tauri-apps/api/event";

/**
 * 在聊天窗口打开某个会话：窗口已加载时直接切换，未加载时等它发出 `chat-ui-window-load` 再切换
 * （与 Ask 窗口「在聊天窗口继续」同一做法）。
 */
export async function openConversationInChat(conversationId: number): Promise<void> {
    const select = () => void emitTo("chat_ui", "select_conversation", String(conversationId));
    const unlisten = await once("chat-ui-window-load", select);
    select();
    try {
        await invoke("open_chat_ui_window");
    } finally {
        // 已打开的窗口不会再发 load 事件；稍后移除一次性监听
        setTimeout(unlisten, 10_000);
    }
}

/** 聊天窗口切到新会话并把 `text` 放进输入框的事件（不自动发送）。 */
export const START_CONVERSATION_WITH_DRAFT_EVENT = "start_conversation_with_draft";

/** 在聊天窗口开一个新会话，输入框预填 `text`（例如合集「在新会话里修改」填入 `#收藏名 `）。 */
export async function startConversationInChat(text: string): Promise<void> {
    const start = () => void emitTo("chat_ui", START_CONVERSATION_WITH_DRAFT_EVENT, { text });
    const unlisten = await once("chat-ui-window-load", start);
    start();
    try {
        await invoke("open_chat_ui_window");
    } finally {
        setTimeout(unlisten, 10_000);
    }
}
