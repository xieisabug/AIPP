import type { FeatureConfig } from "@/hooks/feature/useFeatureConfig";

export const EXPERIMENTAL_CONFIG_DEFAULT_VALUES = {
    dynamic_mcp_loading_enabled: "false",
    mcp_summarizer_model_id: "",
    assistant_summary_enabled: "false",
    assistant_summarizer_model_id: "",
    conversation_summary_enabled: "false",
    conversation_summary_model: "",
    butler_experiment_enabled: "false",
    butler_display_name: "总管家",
    default_home_window: "ask",
    butler_model_id: "",
    butler_feishu_enabled: "false",
    butler_feishu_app_id: "",
    butler_feishu_app_secret: "",
    butler_feishu_base_url: "https://open.feishu.cn",
    butler_feishu_receive_p2p: "true",
    butler_feishu_receive_group: "true",
    butler_feishu_group_require_mention: "true",
    butler_feishu_only_reply_feishu_originated: "false",
    butler_feishu_allowed_open_ids: "",
    butler_feishu_allowed_chat_ids: "",
    context_compaction_enabled: "false",
    context_max_input_tokens: "128000",
    context_compaction_threshold: "0.80",
    context_tail_ratio: "0.30",
    butler_trust_all_workspaces: "false",
    butler_trusted_workspaces: "",
} as const;

export type ExperimentalConfigFormValues = Record<
    keyof typeof EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    string
>;

export type ExperimentalConfigFormState = Record<
    keyof typeof EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    string | boolean
>;

interface SaveFeatureConfigFn {
    (featureCode: string, config: Record<string, unknown>): Promise<unknown>;
}

function parseModelValue(modelValue: string) {
    if (!modelValue) {
        return { model_code: "", provider_id: "" };
    }

    const parts = modelValue.split("%%");
    return {
        model_code: parts[0] || "",
        provider_id: parts[1] || "",
    };
}

export function buildExperimentalConfigFormValues(
    featureConfig: FeatureConfig
): ExperimentalConfigFormValues {
    const summaryConfig = featureConfig.get("conversation_summary");
    const experimentalConfig = featureConfig.get("experimental");

    const experimentalModel = experimentalConfig?.get("conversation_summary_model") || "";
    const experimentalProviderId =
        experimentalConfig?.get("conversation_summary_provider_id") || "";
    const legacyModel = summaryConfig?.get("conversation_summary_model") || "";
    const legacyProviderId = summaryConfig?.get("conversation_summary_provider_id") || "";

    return {
        ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
        dynamic_mcp_loading_enabled:
            experimentalConfig?.get("dynamic_mcp_loading_enabled") || "false",
        mcp_summarizer_model_id: experimentalConfig?.get("mcp_summarizer_model_id") || "",
        assistant_summary_enabled:
            experimentalConfig?.get("assistant_summary_enabled") || "false",
        assistant_summarizer_model_id:
            experimentalConfig?.get("assistant_summarizer_model_id") || "",
        conversation_summary_enabled:
            experimentalConfig?.get("conversation_summary_enabled")
            || summaryConfig?.get("conversation_summary_enabled")
            || "false",
        conversation_summary_model:
            experimentalModel && experimentalProviderId
                ? `${experimentalModel}%%${experimentalProviderId}`
                : legacyModel && legacyProviderId
                    ? `${legacyModel}%%${legacyProviderId}`
                    : "",
        butler_experiment_enabled:
            experimentalConfig?.get("butler_experiment_enabled") || "false",
        butler_display_name: experimentalConfig?.get("butler_display_name") || "总管家",
        default_home_window: experimentalConfig?.get("default_home_window") || "ask",
        butler_model_id: experimentalConfig?.get("butler_model_id") || "",
        butler_feishu_enabled: experimentalConfig?.get("butler_feishu_enabled") || "false",
        butler_feishu_app_id: experimentalConfig?.get("butler_feishu_app_id") || "",
        butler_feishu_app_secret: "",
        butler_feishu_base_url:
            experimentalConfig?.get("butler_feishu_base_url") || "https://open.feishu.cn",
        butler_feishu_receive_p2p:
            experimentalConfig?.get("butler_feishu_receive_p2p") || "true",
        butler_feishu_receive_group:
            experimentalConfig?.get("butler_feishu_receive_group") || "true",
        butler_feishu_group_require_mention:
            experimentalConfig?.get("butler_feishu_group_require_mention") || "true",
        butler_feishu_only_reply_feishu_originated:
            experimentalConfig?.get("butler_feishu_only_reply_feishu_originated") || "false",
        butler_feishu_allowed_open_ids:
            experimentalConfig?.get("butler_feishu_allowed_open_ids") || "",
        butler_feishu_allowed_chat_ids:
            experimentalConfig?.get("butler_feishu_allowed_chat_ids") || "",
        context_compaction_enabled:
            experimentalConfig?.get("context_compaction_enabled") || "false",
        context_max_input_tokens:
            experimentalConfig?.get("context_max_input_tokens") || "128000",
        context_compaction_threshold:
            experimentalConfig?.get("context_compaction_threshold") || "0.80",
        context_tail_ratio: experimentalConfig?.get("context_tail_ratio") || "0.30",
        butler_trust_all_workspaces:
            experimentalConfig?.get("butler_trust_all_workspaces") || "false",
        butler_trusted_workspaces:
            experimentalConfig?.get("butler_trusted_workspaces") || "",
    };
}

export async function saveExperimentalConfigValues(
    saveFeatureConfig: SaveFeatureConfigFn,
    values: Record<string, unknown>
) {
    const conversationSummaryModel = parseModelValue(
        String(values.conversation_summary_model || "")
    );

    await saveFeatureConfig("experimental", {
        dynamic_mcp_loading_enabled: String(values.dynamic_mcp_loading_enabled),
        mcp_summarizer_model_id: String(values.mcp_summarizer_model_id || ""),
        assistant_summary_enabled: String(values.assistant_summary_enabled),
        assistant_summarizer_model_id: String(values.assistant_summarizer_model_id || ""),
        conversation_summary_enabled: String(values.conversation_summary_enabled),
        conversation_summary_model: conversationSummaryModel.model_code,
        conversation_summary_provider_id: conversationSummaryModel.provider_id,
        butler_experiment_enabled: String(values.butler_experiment_enabled),
        butler_display_name: String(values.butler_display_name || "总管家"),
        default_home_window: String(values.default_home_window || "ask"),
        butler_model_id: String(values.butler_model_id || ""),
        butler_feishu_enabled: String(values.butler_feishu_enabled),
        butler_feishu_app_id: String(values.butler_feishu_app_id || ""),
        butler_feishu_base_url: String(
            values.butler_feishu_base_url || "https://open.feishu.cn"
        ),
        butler_feishu_receive_p2p: String(values.butler_feishu_receive_p2p),
        butler_feishu_receive_group: String(values.butler_feishu_receive_group),
        butler_feishu_group_require_mention: String(
            values.butler_feishu_group_require_mention
        ),
        butler_feishu_only_reply_feishu_originated: String(
            values.butler_feishu_only_reply_feishu_originated
        ),
        butler_feishu_allowed_open_ids: String(values.butler_feishu_allowed_open_ids || ""),
        butler_feishu_allowed_chat_ids: String(values.butler_feishu_allowed_chat_ids || ""),
        context_compaction_enabled: String(values.context_compaction_enabled),
        context_max_input_tokens: String(values.context_max_input_tokens || "128000"),
        context_compaction_threshold: String(
            values.context_compaction_threshold || "0.80"
        ),
        context_tail_ratio: String(values.context_tail_ratio || "0.30"),
        butler_trust_all_workspaces: String(values.butler_trust_all_workspaces),
        butler_trusted_workspaces: String(values.butler_trusted_workspaces || ""),
    });
}
