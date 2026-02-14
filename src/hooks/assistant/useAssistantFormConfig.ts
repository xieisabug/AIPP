import { useMemo, useCallback } from "react";
import React from "react";
import { AssistantDetail } from "@/data/Assistant";
import { AssistantFormConfig } from "@/types/forms";
import { validateConfig } from "@/utils/validate";
import AssistantMCPFieldDisplay from "@/components/config/AssistantMCPFieldDisplay";
import AssistantSkillsFieldDisplay from "@/components/config/AssistantSkillsFieldDisplay";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";

interface UseAssistantFormConfigProps {
    currentAssistant: AssistantDetail | null;
    assistantTypeNameMap: Map<number, string>;
    assistantTypeCustomField: Array<{ key: string; value: Record<string, any> }>;
    assistantTypeCustomLabel: Map<string, string>;
    assistantTypeCustomTips: Map<string, string>;
    assistantTypeHideField: Array<string>;
    navigateTo: (menuKey: string) => void;
    onConfigChange: (key: string, value: string | boolean, value_type: string) => void;
    onPromptChange: (value: string) => void;
}

export const useAssistantFormConfig = ({
    currentAssistant,
    assistantTypeNameMap,
    assistantTypeCustomField,
    assistantTypeCustomLabel,
    assistantTypeCustomTips,
    assistantTypeHideField,
    navigateTo,
    onConfigChange,
    onPromptChange,
}: UseAssistantFormConfigProps) => {
    const { getConfigValue } = useFeatureConfig();

    // 处理配置修改
    const handleConfigChange = useCallback(
        (key: string, value: string | boolean, value_type: string) => {
            if (!currentAssistant) return;

            const { isValid, parsedValue } = validateConfig(value, value_type);
            if (!isValid) return;

            onConfigChange(key, parsedValue, value_type);
        },
        [currentAssistant, onConfigChange]
    );

    // 处理模型变化
    const handleModelChange = useCallback(
        (value: string | boolean) => {
            if (!currentAssistant) return;

            // 这里需要外部组件处理模型变化逻辑
            onConfigChange("model", value as string, "string");
        },
        [currentAssistant, onConfigChange]
    );

    // 获取 ACP 配置值的辅助函数
    const getAcpConfigValue = useCallback((configName: string, defaultValue: string) => {
        return currentAssistant?.model_configs?.find(c => c.name === configName)?.value ?? defaultValue;
    }, [currentAssistant]);

    // 生成表单配置
    const formConfig: AssistantFormConfig[] = useMemo(() => {
        if (!currentAssistant) return [];

        const globalDynamicMcpEnabled =
            getConfigValue("experimental", "dynamic_mcp_loading_enabled") === "true";
        const assistantDynamicRaw = currentAssistant.model_configs.find(
            (config) => config.name === "dynamic_mcp_loading_enabled"
        )?.value;
        const assistantDynamicEnabled =
            globalDynamicMcpEnabled &&
            (assistantDynamicRaw == null ||
                (assistantDynamicRaw !== "false" && assistantDynamicRaw !== "0"));

        // ACP 助手类型 (assistant_type === 4) 的专用配置
        if (currentAssistant?.assistant.assistant_type === 4) {
            // 获取当前选择的提供商 ID
            const currentProviderId = currentAssistant?.model.length ?? 0 > 0
                ? currentAssistant.model[0].provider_id.toString()
                : "-1";

            return [
                {
                    key: "assistantType",
                    config: {
                        type: "static" as const,
                        label: "助手类型",
                        value: "ACP 助手",
                    },
                },
                {
                    key: "acp_provider",
                    config: {
                        type: "provider-select" as const,
                        label: "选择提供商",
                        value: currentProviderId,
                        onChange: (value: string | boolean) => {
                            // 保存提供商 ID 到 model 字段的 provider_id 部分
                            // 使用特殊格式 "%%{provider_id}" 让保存逻辑正确处理
                            onConfigChange("model", `%%${value}` as string, "string");
                        },
                    },
                },
                {
                    key: "acp_working_directory",
                    config: {
                        type: "input" as const,
                        label: "工作目录",
                        value: getAcpConfigValue("acp_working_directory", ""),
                        tooltip: "Agent 将在此目录下运行",
                        onChange: (value: string | boolean) =>
                            handleConfigChange("acp_working_directory", value, "string"),
                    },
                },
                {
                    key: "acp_env_vars",
                    config: {
                        type: "textarea" as const,
                        label: "环境变量",
                        value: getAcpConfigValue("acp_env_vars", ""),
                        tooltip: "每行一个，格式: KEY=VALUE",
                        className: "h-32",
                        onChange: (value: string | boolean) =>
                            handleConfigChange("acp_env_vars", value, "string"),
                    },
                },
                {
                    key: "acp_additional_args",
                    config: {
                        type: "input" as const,
                        label: "附加启动参数",
                        value: getAcpConfigValue("acp_additional_args", ""),
                        tooltip: "传递给 CLI 的额外参数，空格分隔",
                        onChange: (value: string | boolean) =>
                            handleConfigChange("acp_additional_args", value, "string"),
                    },
                },
            ];
        }

        // 去重：同名配置只保留最后一次（避免界面重复渲染同一字段）
        const uniqueModelConfigs = (() => {
            const map = new Map<string, typeof currentAssistant.model_configs[number]>();
            for (const cfg of currentAssistant.model_configs ?? []) {
                map.set(cfg.name, cfg);
            }
            return Array.from(map.values());
        })();

        let baseConfigs: AssistantFormConfig[] = [
            {
                key: "assistantType",
                config: {
                    type: "static" as const,
                    label: assistantTypeCustomLabel.get("assistantType") ?? "助手类型",
                    value: assistantTypeNameMap.get(currentAssistant?.assistant.assistant_type ?? 0) ?? "普通对话助手",
                },
            },
            {
                key: "model",
                config: {
                    type: "model-select" as const,
                    label: assistantTypeCustomLabel.get("model") ?? "Model",
                    value:
                        currentAssistant?.model.length ?? 0 > 0
                            ? `${currentAssistant?.model[0].model_code}%%${currentAssistant?.model[0].provider_id}`
                            : "-1",
                    onChange: handleModelChange,
                },
            },
            ...(uniqueModelConfigs ?? [])
                .filter(
                    (config) =>
                        !assistantTypeHideField.includes(config.name) &&
                        !assistantTypeCustomField.find((field) => field.key === config.name) &&
                        config.name !== "reasoning_effort" &&
                        config.name !== "dynamic_mcp_loading_enabled"
                )
                .map((config) => ({
                    key: config.name,
                    config: {
                        type: config.value_type === "boolean" ? ("checkbox" as const) : ("input" as const),
                        label: assistantTypeCustomLabel.get(config.name) ?? config.name,
                        value: config.value_type === "boolean" ? config.value == "true" : config.value,
                        tooltip: assistantTypeCustomTips.get(config.name),
                        onChange: (value: string | boolean) =>
                            handleConfigChange(config.name, value, config.value_type),
                        onBlur: (value: string | boolean) =>
                            handleConfigChange(config.name, value as string, config.value_type),
                    },
                })),
            ...assistantTypeCustomField
                .filter((field) => !assistantTypeHideField.includes(field.key))
                .map((field) => ({
                    key: field.key,
                    config: {
                        ...field.value,
                        type: field.value.type,
                        label: assistantTypeCustomLabel.get(field.key) ?? field.value.label,
                        value: (() => {
                            const config = currentAssistant?.model_configs.find((config) => config.name === field.key);
                            if (field.value.type === "checkbox") {
                                return config?.value === "true";
                            } else if (field.value.type === "static") {
                                return config?.value;
                            } else {
                                return config?.value ?? field.value.value ?? "";
                            }
                        })(),
                        tooltip: assistantTypeCustomTips.get(field.key),
                        onChange: (value: string | boolean) =>
                            handleConfigChange(
                                field.key,
                                value,
                                field.value.type === "checkbox" ? "boolean" : "string"
                            ),
                        onBlur: (value: string | boolean) =>
                            handleConfigChange(
                                field.key,
                                value as string,
                                field.value.type === "checkbox" ? "boolean" : "string"
                            ),
                    },
                })),
        ];

        // reasoning_effort 内置字段
        if (!assistantTypeHideField.includes("reasoning_effort")) {
            const reasoningConfig = currentAssistant?.model_configs.find((c) => c.name === "reasoning_effort");
            baseConfigs.push({
                key: "reasoning_effort",
                config: {
                    type: "select" as const,
                    label: assistantTypeCustomLabel.get("reasoning_effort") ?? "思考级别",
                    value: reasoningConfig?.value ?? "medium",
                    options: [
                        { value: "minimal", label: "最低" },
                        { value: "low", label: "低" },
                        { value: "medium", label: "中" },
                        { value: "high", label: "高" },
                    ],
                    tooltip: assistantTypeCustomTips.get("reasoning_effort"),
                    onChange: (value: string | boolean) =>
                        handleConfigChange("reasoning_effort", value, "string"),
                },
            });
        }

        if (globalDynamicMcpEnabled && !assistantTypeHideField.includes("dynamic_mcp_loading_enabled")) {
            baseConfigs.push({
                key: "dynamic_mcp_loading_enabled",
                config: {
                    type: "switch" as const,
                    label: "MCP 动态加载（实验）",
                    value: assistantDynamicEnabled,
                    tooltip: "开启后采用目录摘要 + 按需加载模式，并隐藏手动 MCP 选择",
                    onChange: (value: string | boolean) =>
                        handleConfigChange("dynamic_mcp_loading_enabled", value, "boolean"),
                },
            });
        }

        if (!assistantTypeHideField.includes("mcp_config") && !assistantDynamicEnabled) {
            baseConfigs.push({
                key: "mcp_config",
                config: {
                    type: "custom" as const,
                    label: "MCP工具",
                    customRender: () => {
                        return React.createElement(AssistantMCPFieldDisplay, {
                            assistantId: currentAssistant?.assistant.id ?? 0,
                            onConfigChange: () => {
                                console.log("MCP configuration changed");
                            },
                            navigateTo: navigateTo,
                        });
                    },
                },
            });
        }

        if (!assistantTypeHideField.includes("skills_config")) {
            baseConfigs.push({
                key: "skills_config",
                config: {
                    type: "custom" as const,
                    label: "Skills",
                    customRender: () => {
                        return React.createElement(AssistantSkillsFieldDisplay, {
                            assistantId: currentAssistant?.assistant.id ?? 0,
                            onConfigChange: () => {
                                console.log("Skills configuration changed");
                            },
                            navigateTo: navigateTo,
                        });
                    },
                },
            });
        }

        if (!assistantTypeHideField.includes("prompt")) {
            baseConfigs.push({
                key: "prompt",
                config: {
                    type: "textarea" as const,
                    label: assistantTypeCustomLabel.get("prompt") ?? "Prompt",
                    className: "h-64",
                    value: currentAssistant?.prompts[0]?.prompt ?? "",
                    onChange: (value: string | boolean) => onPromptChange(value as string),
                },
            });
        }

        return baseConfigs;
    }, [
        currentAssistant,
        assistantTypeNameMap,
        assistantTypeCustomField,
        assistantTypeCustomLabel,
        assistantTypeHideField,
        assistantTypeCustomTips,
        getConfigValue,
        handleConfigChange,
        handleModelChange,
        navigateTo,
        onPromptChange,
        getAcpConfigValue,
    ]);

    return { formConfig };
};
