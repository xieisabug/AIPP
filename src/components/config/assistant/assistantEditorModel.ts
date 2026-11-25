import { invoke } from "@tauri-apps/api/core";
import type { AssistantDetail } from "@/data/Assistant";
import type { LoadedPlugin, PluginAssistantFormFieldContribution } from "@/services/PluginRuntime";
import { isAgentAssistantField } from "@/hooks/assistant/nativeAgentApprovalFields";

/**
 * 助手配置表单的读写规则：编辑页（AssistantConfig）与新增助手向导的「高级与扩展」步骤共用，
 * 保证两处保存出来的助手配置一致。
 */

export type AssistantTypeCustomField = { key: string; value: Record<string, any> };

export type PluginAssistantFormField = PluginAssistantFormFieldContribution & {
    pluginId: number;
    pluginCode: string;
    pluginName: string;
    formKey: string;
};

interface PluginAssistantConfigItem {
    configId: number;
    pluginId: number;
    assistantId: number;
    configKey: string;
    configValue?: string | null;
}

/** Agent 提供商里能读取默认模型列表的通道。 */
export const AGENT_MODEL_OPTION_KINDS = ["codex_app_server", "claude_sdk", "pi", "cursor", "opencode", "kiro", "antigravity"];

export const buildPluginAssistantFieldFormKey = (pluginId: number, fieldKey: string) =>
    `plugin::${pluginId}::${fieldKey}`;

export const collectPluginAssistantFormFields = (pluginList: LoadedPlugin[]): PluginAssistantFormField[] =>
    pluginList.flatMap((plugin) =>
        (plugin.contributions?.assistantFormFields ?? []).map((field) => ({
            ...field,
            pluginId: plugin.pluginId,
            pluginCode: plugin.code,
            pluginName: plugin.name,
            formKey: buildPluginAssistantFieldFormKey(plugin.pluginId, field.key),
        }))
    );

export const normalizePluginAssistantFieldValue = (
    field: PluginAssistantFormFieldContribution,
    rawValue: string | null | undefined,
) => {
    if (field.type === "checkbox" || field.type === "switch") {
        if (rawValue == null) {
            return field.defaultValue === true || field.defaultValue === "true";
        }
        return rawValue === "true";
    }

    if (rawValue == null) {
        if (field.defaultValue === undefined || field.defaultValue === null) {
            return "";
        }
        return String(field.defaultValue);
    }

    return rawValue;
};

/** 读取插件为助手保存的配置，返回以表单键为键的值。 */
export async function fetchPluginAssistantConfigValues(
    fields: PluginAssistantFormField[],
    assistantId: number,
): Promise<Record<string, string | boolean>> {
    if (fields.length === 0) return {};
    const pluginIds = [...new Set(fields.map((field) => field.pluginId))];
    const configGroups = await Promise.all(
        pluginIds.map(async (pluginId) => ({
            pluginId,
            configs: await invoke<PluginAssistantConfigItem[]>("get_plugin_assistant_configs", {
                pluginId,
                assistantId,
            }),
        }))
    );
    const configMap = new Map<number, PluginAssistantConfigItem[]>(
        configGroups.map((entry) => [entry.pluginId, entry.configs])
    );
    const values: Record<string, string | boolean> = {};
    fields.forEach((field) => {
        const rawValue = configMap
            .get(field.pluginId)
            ?.find((item) => item.configKey === field.key)
            ?.configValue;
        values[field.formKey] = normalizePluginAssistantFieldValue(field, rawValue);
    });
    return values;
}

export async function savePluginAssistantConfigValues(
    fields: PluginAssistantFormField[],
    assistantId: number,
    values: Record<string, any>,
): Promise<void> {
    await Promise.all(
        fields.map((field) =>
            invoke("set_plugin_assistant_config", {
                pluginId: field.pluginId,
                assistantId,
                key: field.key,
                value: values[field.formKey] == null ? null : String(values[field.formKey]),
            })
        )
    );
}

/** 助手详情 → 表单值（与编辑页加载助手时一致）。 */
export function buildAssistantFormValues(
    detail: AssistantDetail,
    customFields: AssistantTypeCustomField[],
    pluginValues: Record<string, string | boolean>,
): Record<string, any> {
    const findConfig = (name: string) => detail.model_configs.find((config) => config.name === name);
    return {
        assistantType: detail.assistant.assistant_type,
        model:
            detail.model.length > 0
                ? `${detail.model[0].model_code}%%${detail.model[0].provider_id}`
                : "-1",
        agent_model:
            detail.model[0]?.model_code && detail.model[0]?.provider_id > 0
                ? `${detail.model[0].model_code}%%${detail.model[0].provider_id}`
                : "",
        prompt: detail.prompts[0]?.prompt ?? "",
        tool_review_mode: detail.assistant.assistant_type === 4 ? "" : "off",
        // Match the switches shown for assistants created before these settings existed.
        ...(detail.assistant.assistant_type !== 4 && { use_native_toolcall: false }),
        dynamic_mcp_loading_enabled: true,
        ...detail.model_configs.reduce((acc, config) => {
            acc[config.name] = config.value_type === "boolean" ? config.value == "true" : config.value;
            return acc;
        }, {} as Record<string, any>),
        // assistant_model.provider_id 是 Agent 提供商绑定的权威来源。
        ...(detail.assistant.assistant_type === 4 ? { claude_effort: findConfig("claude_effort")?.value || "inherit" } : {}),
        // 放在旧 model_configs 之后，避免遗留 acp_provider 覆盖当前绑定。
        acp_provider:
            detail.assistant.assistant_type === 4 &&
            detail.model.length > 0 &&
            detail.model[0].provider_id > 0
                ? detail.model[0].provider_id.toString()
                : "-1",
        codex_approval_policy: findConfig("codex_approval_policy")?.value || "default",
        codex_sandbox: findConfig("codex_sandbox")?.value || "default",
        codex_approvals_reviewer: findConfig("codex_approvals_reviewer")?.value || "default",
        ...customFields.reduce((acc, field) => {
            acc[field.key] =
                field.value.type === "checkbox"
                    ? findConfig(field.key)?.value === "true"
                    : findConfig(field.key)?.value ?? "";
            return acc;
        }, {} as Record<string, any>),
        ...pluginValues,
    };
}

/** 表单值 → 待保存的助手详情（`save_assistant` 的入参）。 */
export function buildAssistantSaveDetail(
    currentAssistant: AssistantDetail,
    values: Record<string, any>,
    customFields: AssistantTypeCustomField[],
): AssistantDetail {
    const isAgent = currentAssistant.assistant.assistant_type === 4;
    const model = (() => {
        if (Number(values.assistantType) === 4) {
            const providerId = parseInt(String(values.acp_provider ?? ""), 10);
            if (Number.isFinite(providerId) && providerId > 0) {
                const existingModel = currentAssistant.model[0];
                const selectedAgentModel = String(values.agent_model ?? "").split("%%")[0];
                return [{
                    id: existingModel?.id ?? 0,
                    assistant_id: existingModel?.assistant_id ?? currentAssistant.assistant.id,
                    model_code: selectedAgentModel || existingModel?.model_code || "",
                    provider_id: providerId,
                    alias: "",
                }];
            }
            return currentAssistant.model;
        }

        // 如果模型选择是 "-1" 或无效，保留原有模型信息
        const modelValue = values.model;
        const modelParts = modelValue?.split("%%") || [];
        const hasValidModel = modelParts.length === 2 && modelValue !== "-1";
        if (hasValidModel) {
            return [{
                ...currentAssistant.model[0],
                model_code: modelParts[0],
                provider_id: parseInt(modelParts[1]) || 0,
                alias: "",
            }];
        }
        // 没有有效选择时保留原有模型配置（可能为空）
        return currentAssistant.model;
    })();

    const modelConfigs = Object.entries(values)
        .filter(
            ([key]) =>
                !key.startsWith("plugin::")
                && !key.startsWith("acp_session_")
                && key !== "assistantType"
                && key !== "model"
                && key !== "agent_model"
                && key !== "acp_provider"
                && key !== "prompt"
                && key !== "mcp_config"
                && key !== "skills_config"
        )
        .filter(([key]) => {
            const config = currentAssistant.model_configs.find((config) => config.name === key);
            if (key === "use_native_toolcall" && !config && values[key] === false) return false;
            if (isAgent && key === "tool_review_mode" && !values[key]) return false;
            if (key === "claude_effort" && values[key] === "inherit" && !config) return false;
            // ACP/Codex 助手专用字段（assistant_type === 4）
            if (isAgentAssistantField(key) && isAgent) return true;
            // 内置字段允许保存
            if (key === "reasoning_effort" || key === "use_native_toolcall" || key === "tool_review_mode") return true;
            // 插件自定义字段直接允许保存
            if (customFields.some((field) => field.key === key)) return true;
            return (
                config &&
                config.value_type &&
                config.value_type !== "static" &&
                config.value_type !== "button" &&
                config.value_type !== "custom"
            );
        })
        .map(([key, value]) => {
            const config = currentAssistant.model_configs.find((config) => config.name === key);
            const customField = customFields.find((field) => field.key === key);

            let valueType = config?.value_type ?? "string";
            if (isAgentAssistantField(key) && isAgent) {
                valueType = "string";
            } else if (key === "use_native_toolcall") {
                valueType = "boolean";
            } else if (key === "tool_review_mode") {
                valueType = "string";
            } else if (customField) {
                // 根据插件字段的类型映射到数据库的 value_type
                const fieldType = customField.value.type;
                valueType = fieldType === "checkbox" || fieldType === "switch" ? "boolean" : "string";
            } else if (key === "reasoning_effort") {
                valueType = "string";
            }

            // Codex 字段的 "default" 表示跟随提供商配置，落库时归一化为空字符串
            const inheritsProvider = (key.startsWith("codex_") && value === "default")
                || (key === "claude_effort" && value === "inherit");
            const normalizedValue = inheritsProvider ? "" : value;

            return {
                name: key,
                value: normalizedValue != null ? normalizedValue.toString() : null,
                value_type: valueType,
                id: config?.id ?? 0,
                assistant_id: currentAssistant.assistant.id,
                assistant_model_id: currentAssistant.model[0]?.id ?? 0,
            };
        });

    return {
        ...currentAssistant,
        assistant: {
            ...currentAssistant.assistant,
            assistant_type: values.assistantType,
        },
        model,
        // 与原实现一致：value 为 null 时交给后端处理
        model_configs: modelConfigs as AssistantDetail["model_configs"],
        prompts: [
            {
                ...currentAssistant.prompts[0],
                prompt: values.prompt,
            },
        ],
    };
}
