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
