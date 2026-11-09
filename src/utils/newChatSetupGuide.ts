/**
 * 新会话首次使用引导（LeaderAI #155）。
 *
 * 「有提供商」不等于「可以发送」：提供商可能全被停用、启用后没有模型，
 * 或者当前助手绑定的模型已删除 / 所属提供商被停用。这里统一判定，给出对应引导。
 */

export type SetupGuideKind =
    | "no_provider"
    | "providers_disabled"
    | "no_models"
    | "no_assistant"
    | "assistant_model_invalid";

export interface SetupGuide {
    kind: SetupGuideKind;
    message: string;
    /** 主操作：去配置提供商，还是去配置助手 */
    primary: "providers" | "assistant";
}

export interface SetupGuideInput {
    providers: Array<{ id: number; is_enabled?: boolean }>;
    /** get_models_for_select 的结果：只包含已启用提供商下的模型 */
    models: Array<{ code: string; llm_provider_id: number }>;
    assistantCount: number;
    /**
     * 当前所选普通助手绑定的模型。undefined 表示不检查
     * （未选助手、Agent 助手走自己的模型列表、或详情还没读到）。
     * null 表示助手没有绑定任何模型。
     */
    assistantModel?: { provider_id: number; model_code: string } | null;
}

export function resolveSetupGuide(input: SetupGuideInput): SetupGuide | null {
    const { providers, models, assistantCount, assistantModel } = input;
    if (providers.length === 0) {
        return { kind: "no_provider", message: "还没有模型提供商，请先添加并填写密钥", primary: "providers" };
    }
    if (!providers.some((provider) => provider.is_enabled !== false)) {
        return { kind: "providers_disabled", message: "模型提供商都已停用，请启用至少一个", primary: "providers" };
    }
    if (models.length === 0) {
        return { kind: "no_models", message: "已启用的提供商下还没有模型，请先获取或添加模型", primary: "providers" };
    }
    if (assistantCount === 0) {
        return { kind: "no_assistant", message: "还没有可用的助手，请先创建助手", primary: "assistant" };
    }
    if (assistantModel !== undefined) {
        const usable = assistantModel != null && models.some(
            (model) => model.llm_provider_id === assistantModel.provider_id && model.code === assistantModel.model_code,
        );
        if (!usable) {
            return {
                kind: "assistant_model_invalid",
                message: "当前助手的模型不可用（已删除或提供商已停用），请为助手重新选择模型",
                primary: "assistant",
            };
        }
    }
    return null;
}
