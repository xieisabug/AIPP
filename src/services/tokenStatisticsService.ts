import { invoke } from "@tauri-apps/api/core";
import type {
    ConversationTokenStats,
    MessageTokenStats,
} from "@/data/Conversation";

/**
 * Token统计服务
 */
export const tokenStatisticsService = {
    /**
     * 获取对话的token统计信息
     */
    async getConversationTokenStats(
        conversationId: string
    ): Promise<ConversationTokenStats> {
        try {
            const stats = await invoke<ConversationTokenStats>(
                "get_conversation_token_stats",
                { conversationId: parseInt(conversationId) }
            );

            // 计算百分比
            if (stats.total_tokens > 0) {
                stats.by_model = stats.by_model.map((model) => ({
                    ...model,
                    percentage: (model.total_tokens / stats.total_tokens) * 100,
                }));
            }

            return stats;
        } catch (error) {
            console.error("Failed to get conversation token stats:", error);
            throw error;
        }
    },

    /**
     * 获取单个消息的token统计信息
     */
    async getMessageTokenStats(messageId: number): Promise<MessageTokenStats> {
        try {
            return await invoke<MessageTokenStats>("get_message_token_stats", {
                messageId,
            });
        } catch (error) {
            console.error("Failed to get message token stats:", error);
            throw error;
        }
    },
};
