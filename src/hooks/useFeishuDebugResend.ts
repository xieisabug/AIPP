import { useCallback, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";

interface FeishuDebugSendResult {
    external_message_id: string;
    payload_type: string;
    part_count: number;
    interactive_part_count: number;
    text_part_count: number;
    delivery_mode: string;
    reply_to_message_id?: string | null;
    target_type?: string | null;
    target_id?: string | null;
    rendered_text: string;
    interactive_error?: string | null;
    interactive_card?: unknown;
}

export function useFeishuDebugResend() {
    const [pendingMessageId, setPendingMessageId] = useState<number | null>(null);

    const resendMessageToFeishuDebug = useCallback(async (messageId: number) => {
        if (pendingMessageId !== null) {
            return;
        }

        setPendingMessageId(messageId);
        try {
            const result = await invoke<FeishuDebugSendResult>("debug_resend_message_to_feishu", {
                messageId,
                message_id: messageId,
            });

            console.debug("[FeishuDebugResend]", result);

            const descriptionParts = [
                `发送类型：${result.payload_type}`,
                `投递方式：${result.delivery_mode === "reply" ? "回复" : "直发"}`,
            ];
            if (result.part_count > 1) {
                descriptionParts.push(
                    `分片：${result.part_count}（卡片 ${result.interactive_part_count} / 文本 ${result.text_part_count}）`,
                );
            }
            if (result.reply_to_message_id) {
                descriptionParts.push(`reply_to：${result.reply_to_message_id}`);
            }
            if (result.target_type && result.target_id) {
                descriptionParts.push(`目标：${result.target_type}=${result.target_id}`);
            }
            if (result.interactive_error) {
                descriptionParts.push(`interactive失败：${result.interactive_error}`);
            }

            toast.success("已重新发送到飞书", {
                description: descriptionParts.join(" | "),
            });
        } catch (error) {
            toast.error(`重新发送到飞书失败: ${error instanceof Error ? error.message : String(error)}`);
        } finally {
            setPendingMessageId((current) => (current === messageId ? null : current));
        }
    }, [pendingMessageId]);

    return {
        pendingMessageId,
        resendMessageToFeishuDebug,
    };
}
