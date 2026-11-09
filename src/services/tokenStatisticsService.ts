import { invoke } from "@tauri-apps/api/core";
import type {
    ConversationTokenStats,
    MessageTokenStats,
} from "@/data/Conversation";

/** 计算各模型占总量的百分比，供模型卡片展示 */
function withModelPercentages(stats: ConversationTokenStats): ConversationTokenStats {
    if (stats.total_tokens <= 0) {
        return stats;
    }
    return {
        ...stats,
        by_model: stats.by_model.map((model) => ({
            ...model,
            percentage: (model.total_tokens / stats.total_tokens) * 100,
        })),
    };
}

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
        const stats = await invoke<ConversationTokenStats>(
            "get_conversation_token_stats",
            { conversationId: parseInt(conversationId) }
        );
        return withModelPercentages(stats);
    },

    /**
     * 获取管家群聊的token统计信息（成员执行会话 + 调度模型调用）
     */
    async getGroupTokenStats(groupId: number): Promise<ConversationTokenStats> {
        const stats = await invoke<ConversationTokenStats>(
            "get_butler_group_token_stats",
            { groupId }
        );
        return withModelPercentages(stats);
    },

    /**
     * 获取单个消息的token统计信息
     */
    async getMessageTokenStats(messageId: number): Promise<MessageTokenStats> {
        return invoke<MessageTokenStats>("get_message_token_stats", {
            messageId,
        });
    },
};

/** Tauri 命令失败时抛出的是后端返回的字符串，不是 Error；统一取出可读的具体原因。 */
export function describeInvokeError(error: unknown, fallback: string): string {
    if (typeof error === "string" && error.trim()) return error;
    if (error instanceof Error && error.message) return error.message;
    if (error && typeof error === "object" && "message" in error) {
        const message = (error as { message?: unknown }).message;
        if (typeof message === "string" && message.trim()) return message;
    }
    return `${fallback}：${String(error)}`;
}
