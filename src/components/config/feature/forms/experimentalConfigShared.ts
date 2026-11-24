import type { FeatureConfig } from "@/hooks/feature/useFeatureConfig";
import {
    buildButlerWorkspaceConfig,
    BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
    serializeButlerWorkspaceConfig,
} from "@/components/butler/butlerWorkspaceConfig";

export const EXPERIMENTAL_CONFIG_DEFAULT_VALUES = {
    dynamic_mcp_loading_enabled: "false",
    mcp_summarizer_model_id: "",
    assistant_summary_enabled: "false",
    assistant_summarizer_model_id: "",
    conversation_summary_enabled: "false",
    conversation_summary_model: "",
    butler_experiment_enabled: "false",
    butler_task_dispatch_enabled: "true",
    butler_peer_collaboration_enabled: "true",
    butler_work_continuation_enabled: "true",

    butler_display_name: "总管家",
    butler_model_id: "",
    butler_helper_model_id: "",
    butler_group_dispatch_model_id: "",
    butler_main_workspace_path: "",
    butler_main_workspace_description: BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
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

export interface SaveFeatureConfigFn {
    (featureCode: string, config: Record<string, unknown>, partial?: boolean): Promise<unknown>;
}

function parseModelValue(modelValue: string) {
    const separator = modelValue.lastIndexOf("%%");
    const providerId = modelValue.slice(separator + 2);
    if (separator <= 0 || !/^[1-9]\d*$/.test(providerId)) {
        return { model_code: "", provider_id: "" };
    }

    return {
        model_code: modelValue.slice(0, separator),
        provider_id: providerId,
    };
}

export function buildExperimentalConfigFormValues(
    featureConfig: FeatureConfig
): ExperimentalConfigFormValues {
    const summaryConfig = featureConfig.get("conversation_summary");
    const experimentalConfig = featureConfig.get("experimental");

    // Use the same group as the summary scheduler, including an explicitly cleared selection.
    const summary = ["conversation_summary_enabled", "conversation_summary_model", "conversation_summary_provider_id"]
        .some((key) => experimentalConfig?.has(key)) ? experimentalConfig : summaryConfig;
    const model = summary?.get("conversation_summary_model") || "";
    const provider = summary?.get("conversation_summary_provider_id") || "";
    const selection = model && provider ? `${model}%%${provider}` : model;
    const workspaceConfig = buildButlerWorkspaceConfig({
        mainWorkspacePath: experimentalConfig?.get("butler_main_workspace_path"),
        mainWorkspaceDescription: experimentalConfig?.get("butler_main_workspace_description"),
        trustedWorkspacesRaw: experimentalConfig?.get("butler_trusted_workspaces") || "",
    });

    return {
        ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
        dynamic_mcp_loading_enabled:
            experimentalConfig?.get("dynamic_mcp_loading_enabled") || "false",
        mcp_summarizer_model_id: experimentalConfig?.get("mcp_summarizer_model_id") || "",
        assistant_summary_enabled:
            experimentalConfig?.get("assistant_summary_enabled") || "false",
        assistant_summarizer_model_id:
            experimentalConfig?.get("assistant_summarizer_model_id") || "",
        conversation_summary_enabled: summary?.get("conversation_summary_enabled") || "false",
        conversation_summary_model: parseModelValue(selection).provider_id ? selection : "",
        butler_experiment_enabled:
            experimentalConfig?.get("butler_experiment_enabled") || "false",
        butler_task_dispatch_enabled: experimentalConfig?.get("butler_task_dispatch_enabled") ?? "true",
        butler_peer_collaboration_enabled: experimentalConfig?.get("butler_peer_collaboration_enabled") ?? "true",
        butler_work_continuation_enabled: experimentalConfig?.get("butler_work_continuation_enabled") ?? "true",
        butler_display_name: experimentalConfig?.get("butler_display_name") || "总管家",
        butler_model_id: experimentalConfig?.get("butler_model_id") || "",
        butler_helper_model_id: experimentalConfig?.get("butler_helper_model_id") || "",
        butler_group_dispatch_model_id:
            experimentalConfig?.get("butler_group_dispatch_model_id") || "",
        butler_main_workspace_path: workspaceConfig.mainWorkspace?.path || "",
        butler_main_workspace_description:
            workspaceConfig.mainWorkspace?.description || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
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
        butler_trust_all_workspaces:
            experimentalConfig?.get("butler_trust_all_workspaces") || "false",
        butler_trusted_workspaces:
            workspaceConfig.trustedWorkspaces.length > 0
                ? JSON.stringify(workspaceConfig.trustedWorkspaces)
                : "",
    };
}

export async function saveExperimentalConfigValues(
    saveFeatureConfig: SaveFeatureConfigFn,
    values: Record<string, unknown>,
    scope: "all" | "butler" = "all",
) {
    const conversationSummaryModel = parseModelValue(
        String(values.conversation_summary_model || "")
    );
    const workspaceConfig = serializeButlerWorkspaceConfig({
        mainWorkspacePath: String(values.butler_main_workspace_path || ""),
        mainWorkspaceDescription: String(values.butler_main_workspace_description || ""),
        trustedWorkspaces: buildButlerWorkspaceConfig({
            mainWorkspacePath: String(values.butler_main_workspace_path || ""),
            mainWorkspaceDescription: String(values.butler_main_workspace_description || ""),
            trustedWorkspacesRaw: String(values.butler_trusted_workspaces || ""),
        }).trustedWorkspaces,
    });

    const butler = {
        butler_experiment_enabled: String(values.butler_experiment_enabled),
        butler_task_dispatch_enabled: String(values.butler_task_dispatch_enabled),
        butler_peer_collaboration_enabled: String(values.butler_peer_collaboration_enabled),
        butler_work_continuation_enabled: String(values.butler_work_continuation_enabled),
        butler_display_name: String(values.butler_display_name || "总管家"),
        butler_model_id: String(values.butler_model_id || ""),
        butler_helper_model_id: String(values.butler_helper_model_id || ""),
        butler_group_dispatch_model_id: String(values.butler_group_dispatch_model_id || ""),
        butler_main_workspace_path: workspaceConfig.mainWorkspacePath,
        butler_main_workspace_description: workspaceConfig.mainWorkspaceDescription,
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
        butler_trust_all_workspaces: String(values.butler_trust_all_workspaces),
        butler_trusted_workspaces: workspaceConfig.trustedWorkspacesRaw,
    };
    if (scope === "butler") {
        await saveFeatureConfig("experimental", butler, true);
        return;
    }
    await saveFeatureConfig("experimental", {
        ...butler,
        dynamic_mcp_loading_enabled: String(values.dynamic_mcp_loading_enabled),
        mcp_summarizer_model_id: String(values.mcp_summarizer_model_id || ""),
        assistant_summary_enabled: String(values.assistant_summary_enabled),
        assistant_summarizer_model_id: String(values.assistant_summarizer_model_id || ""),
        conversation_summary_enabled: String(values.conversation_summary_enabled),
        conversation_summary_model: conversationSummaryModel.model_code,
        conversation_summary_provider_id: conversationSummaryModel.provider_id,
    }, true);
}
