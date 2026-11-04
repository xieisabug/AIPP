import { getErrorMessage } from "@/utils/error";

/** 与后端 `NO_MODEL_CONFIGURED_MESSAGE` 保持一致。 */
export const NO_MODEL_CONFIGURED_MESSAGE =
    "当前助手尚未配置模型，请先在「设置 → 模型提供商」中添加模型，并在「个人助手」中为该助手选择模型后重试。";

/**
 * ask_ai 的返回：对话已创建但在调用模型前失败（如助手未配置模型）时，
 * 后端已把错误写进对话，并在 `error_message` 中带回说明。
 */
export interface AskAiResponse {
    conversation_id: number;
    request_prompt_result_with_context?: string;
    error_message?: string | null;
}

/** 把 ask_ai 的 reject 值（Tauri AppError 序列化结果等）转成给用户看的文案。 */
export function describeAskAiError(error: unknown): string {
    const message = getErrorMessage(error);
    if (message === "NoModelFound") {
        return NO_MODEL_CONFIGURED_MESSAGE;
    }
    return message;
}
