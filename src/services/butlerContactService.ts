import { invoke } from "@tauri-apps/api/core";

/** Clear only the assistant's Butler private conversation, never ordinary chat or tasks. */
export async function clearButlerContactHistory(assistantId: number): Promise<number | null> {
    let stage = "读取联系人私聊";
    let conversationId: number | null = null;
    try {
        const contacts = await invoke<Array<{ assistantId: number; conversationId: number }>>("list_butler_contact_previews");
        conversationId = contacts.find(contact => contact.assistantId === assistantId)?.conversationId ?? null;
        if (conversationId === null) return null;
        stage = "停止私聊执行";
        await invoke("cancel_ai", { conversationId });
        stage = "删除私聊历史";
        await invoke("delete_conversation", { conversationId });
        return conversationId;
    } catch (error) {
        throw new Error(`${stage}失败（助手 ${assistantId}，会话 ${conversationId ?? "尚未取得"}）：${String(error)}`);
    }
}
