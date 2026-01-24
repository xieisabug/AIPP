import { invoke } from "@tauri-apps/api/core";
import { ConversationSearchHit } from "../data/Conversation";

export async function searchConversations(
    query: string,
    limit: number = 50,
    offset: number = 0,
): Promise<ConversationSearchHit[]> {
    const trimmed = query.trim();
    if (!trimmed) {
        return [];
    }
    return invoke<ConversationSearchHit[]>("search_conversations", { query: trimmed, limit, offset });
}
