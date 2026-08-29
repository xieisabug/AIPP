import React, { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { AssistantDetail, AssistantListItem } from "../../data/Assistant";
import { useAssistantListListener } from "../../hooks/useAssistantListListener";
import { Bot, Settings, User, Download } from "lucide-react";
import { Button } from "../ui/button";
import { Tooltip, TooltipTrigger, TooltipContent } from "../ui/tooltip";
import { useForm } from "react-hook-form";
import { validateConfig } from "../../utils/validate";
import { PinyinFilter, usePinyinReady } from "../../utils/pinyinFilter";
import AddAssistantDialog from "./AddAssistantDialog";

// 导入公共组件
import { ConfigPageLayout, SidebarList, ListItemButton, EmptyState, SelectOption } from "../common";

// 导入新的 hooks 和组件
import { useAssistantTypePlugin } from "@/hooks/assistant/useAssistantTypePlugin";
import { useAssistantOperations } from "@/hooks/assistant/useAssistantOperations";
import { useAssistantFormConfig } from "@/hooks/assistant/useAssistantFormConfig";
import { useDialogStates } from "@/hooks/assistant/useDialogStates";
import { AssistantFormRenderer } from "./assistant/AssistantFormRenderer";
import { AssistantDialogs } from "./assistant/AssistantDialogs";
import { AssistantConfigApi } from "@/types/forms";
import type { LoadedPlugin, PluginAssistantFormFieldContribution } from "@/services/PluginRuntime";
import { getErrorMessage } from "@/utils/error";
import { resolveAgentDefaultSelection } from "@/utils/agentDefaults";
import type { AgentModelOption } from "@/utils/agentDefaults";

interface AssistantConfigProps {
    pluginList: LoadedPlugin[];
    navigateTo: (menuKey: string) => void;
}

interface PluginAssistantConfigItem {
    configId: number;
    pluginId: number;
    assistantId: number;
    configKey: string;
    configValue?: string | null;
}

const buildPluginAssistantFieldFormKey = (pluginId: number, fieldKey: string) =>
    `plugin::${pluginId}::${fieldKey}`;

const normalizePluginAssistantFieldValue = (
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
const AssistantConfig: React.FC<AssistantConfigProps> = ({ pluginList, navigateTo }) => {
    const form = useForm();
    const [searchQuery, setSearchQuery] = useState('');
    // 有搜索输入时才动态加载 pinyin-pro，加载完成后触发重新过滤
    const pinyinReady = usePinyinReady(searchQuery.trim().length > 0);
    const [pluginAssistantConfigValues, setPluginAssistantConfigValues] = useState<Record<string, string | boolean>>({});
    const [agentModelOptions, setAgentModelOptions] = useState<AgentModelOption[]>([]);
    const [agentModelLoading, setAgentModelLoading] = useState(false);
    const [agentModelError, setAgentModelError] = useState<string | null>(null);

    const {
        assistantTypes,
        assistantTypePluginMap,
        assistantTypeNameMap,
        assistantTypeCustomField,
        setAssistantTypeCustomField,
        assistantTypeCustomLabel,
        assistantTypeCustomTips,
        assistantTypeHideField,
        assistantTypeApi,
    } = useAssistantTypePlugin(pluginList);

    const {
        assistants,
        currentAssistant,
        setAssistants,
        setCurrentAssistant,
        saveAssistant,
        copyAssistant,
        deleteAssistant,
        loadAssistants,
        loadAssistantDetail,
        shareAssistant,
        importAssistant,
        updateAssistantInfo,
        addAssistant,
    } = useAssistantOperations();

    const {
        dialogStates,
        shareCode,
        openConfirmDeleteDialog,
        closeConfirmDeleteDialog,
        openUpdateFormDialog,
        closeUpdateFormDialog,
        openShareDialog,
        closeShareDialog,
        openImportDialog,
        closeImportDialog,
    } = useDialogStates();

    const pluginAssistantFormFields = useMemo(
        () =>
            pluginList.flatMap((plugin) =>
                (plugin.contributions?.assistantFormFields ?? []).map((field) => ({
                    ...field,
                    pluginId: plugin.pluginId,
                    pluginCode: plugin.code,
                    pluginName: plugin.name,
                    formKey: buildPluginAssistantFieldFormKey(plugin.pluginId, field.key),
                }))
            ),
        [pluginList]
    );

    const loadPluginAssistantConfigValues = useCallback(
        async (assistantId: number) => {
            if (pluginAssistantFormFields.length === 0) {
                setPluginAssistantConfigValues({});
                return {};
            }

            const pluginIds = [...new Set(pluginAssistantFormFields.map((field) => field.pluginId))];
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
            const nextValues: Record<string, string | boolean> = {};
            pluginAssistantFormFields.forEach((field) => {
                const rawValue = configMap
                    .get(field.pluginId)
                    ?.find((item) => item.configKey === field.key)
                    ?.configValue;
                nextValues[field.formKey] = normalizePluginAssistantFieldValue(field, rawValue);
            });
            setPluginAssistantConfigValues(nextValues);
            return nextValues;
        },
        [pluginAssistantFormFields]
    );

    // 助手配置 API
    const assistantConfigApi: AssistantConfigApi = useMemo(
        () => ({
            clearFieldValue: function (fieldName: string): void {
                handleConfigChange(fieldName, "", "");
            },
            changeFieldValue: function (fieldName: string, value: any, valueType: string): void {
                console.log("changeFieldValue", fieldName, value, valueType);
                handleConfigChange(fieldName, value, valueType);
            },
        }),
        [currentAssistant]
    );

    const applyAgentModelSelection = useCallback(
        (modelValue: string, effort: string) => {
            const [modelCode, providerValue] = modelValue.split("%%");
            const providerId = Number.parseInt(providerValue, 10);
            form.setValue("agent_model", modelValue);
            form.setValue("reasoning_effort", effort);
            setCurrentAssistant((previous) => {
                if (!previous || !Number.isFinite(providerId) || providerId <= 0) return previous;
                const existingModel = previous.model[0];
                const nextModel = {
                    id: existingModel?.id ?? 0,
                    assistant_id: existingModel?.assistant_id ?? previous.assistant.id,
                    model_code: modelCode,
                    provider_id: providerId,
                    alias: existingModel?.alias ?? "",
                };
                const configIndex = previous.model_configs.findIndex(
                    (config) => config.name === "reasoning_effort"
                );
                const nextConfig = {
                    id: configIndex >= 0 ? previous.model_configs[configIndex].id : 0,
                    assistant_id: previous.assistant.id,
                    assistant_model_id: existingModel?.id ?? 0,
                    name: "reasoning_effort",
                    value: effort,
                    value_type: "string",
                };
                const modelConfigs = configIndex >= 0
                    ? previous.model_configs.map((config, index) => index === configIndex ? nextConfig : config)
                    : [...previous.model_configs, nextConfig];
                return { ...previous, model: [nextModel], model_configs: modelConfigs };
            });
        },
        [form, setCurrentAssistant]
    );

    const loadAgentModelOptions = useCallback(
        async (assistantDetail: AssistantDetail, providerId: number | null = null) => {
            const selectedProviderId = providerId ?? assistantDetail.model[0]?.provider_id ?? 0;
            if (assistantDetail.assistant.assistant_type !== 4 || selectedProviderId <= 0) {
                setAgentModelOptions([]);
                setAgentModelError(null);
                return;
            }
            setAgentModelLoading(true);
            setAgentModelError(null);
            try {
                const options = await invoke<AgentModelOption[]>("get_agent_model_options", {
                    assistantId: assistantDetail.assistant.id,
                    providerId,
                });
                const useConfiguredValues = providerId == null
                    || providerId === assistantDetail.model[0]?.provider_id;
                const configuredModel = useConfiguredValues
                    ? assistantDetail.model[0]?.model_code
                    : null;
                const configuredEffort = useConfiguredValues
                    ? assistantDetail.model_configs.find((config) => config.name === "reasoning_effort")?.value
                    : null;
                const selection = resolveAgentDefaultSelection(
                    options,
                    configuredModel,
                    configuredEffort,
                );
                setAgentModelOptions(options);
                if (selection.model) {
                    applyAgentModelSelection(selection.model.code, selection.effort);
                }
            } catch (error) {
                const message = `无法读取 Agent 默认配置：${getErrorMessage(error)}`;
                setAgentModelOptions([]);
                setAgentModelError(message);
                toast.error(message);
            } finally {
                setAgentModelLoading(false);
            }
        },
        [applyAgentModelSelection]
    );

    const handleAgentProviderChange = useCallback(
        (providerValue: string, apiType?: string) => {
            const providerId = Number.parseInt(providerValue, 10);
            form.setValue("acp_provider", providerValue);
            setAgentModelOptions([]);
            setAgentModelError(null);
            setCurrentAssistant((previous) => {
                if (!previous || !Number.isFinite(providerId) || providerId <= 0) return previous;
                const existingModel = previous.model[0];
                return {
                    ...previous,
                    model: [{
                        id: existingModel?.id ?? 0,
                        assistant_id: existingModel?.assistant_id ?? previous.assistant.id,
                        model_code: "",
                        provider_id: providerId,
                        alias: existingModel?.alias ?? "",
                    }],
                };
            });
            if (apiType === "codex_app_server") {
                form.setValue("codex_approval_policy", "default");
                form.setValue("codex_sandbox", "default");
                form.setValue("codex_approvals_reviewer", "default");
            }
            if (
                apiType === "codex_app_server"
                && currentAssistant
                && Number.isFinite(providerId)
                && providerId > 0
            ) {
                void loadAgentModelOptions(currentAssistant, providerId);
            }
        },
        [currentAssistant, form, loadAgentModelOptions, setCurrentAssistant]
    );

    // 初始化助手列表
    useEffect(() => {
        loadAssistants().then((assistantList) => {
            if (assistantList.length) {
                handleChooseAssistant(assistantList[0]);
            }
        });
    }, [loadAssistants]);
    // 选择助手
    const handleChooseAssistant = useCallback(
        (assistant: AssistantListItem) => {
            if (!currentAssistant || currentAssistant.assistant.id !== assistant.id) {
                loadAssistantDetail(assistant.id).then(async (assistantDetail) => {
                    const pluginConfigValues = await loadPluginAssistantConfigValues(
                        assistantDetail.assistant.id
                    );
                    form.reset({
                        assistantType: assistantDetail.assistant.assistant_type,
                        model:
                            assistantDetail.model.length > 0
                                ? `${assistantDetail.model[0].model_code}%%${assistantDetail.model[0].provider_id}`
                                : "-1",
                        agent_model:
                            assistantDetail.model[0]?.model_code && assistantDetail.model[0]?.provider_id > 0
                                ? `${assistantDetail.model[0].model_code}%%${assistantDetail.model[0].provider_id}`
                                : "",
                        prompt: assistantDetail.prompts[0].prompt,
                        ...assistantDetail.model_configs.reduce((acc, config) => {
                            acc[config.name] = config.value_type === "boolean" ? config.value == "true" : config.value;
                            return acc;
                        }, {} as Record<string, any>),
                        // assistant_model.provider_id 是 Agent 提供商绑定的权威来源。
                        // 放在旧 model_configs 之后，避免遗留 acp_provider 覆盖当前绑定。
                        acp_provider:
                            assistantDetail.assistant.assistant_type === 4 &&
                            assistantDetail.model.length > 0 &&
                            assistantDetail.model[0].provider_id > 0
                                ? assistantDetail.model[0].provider_id.toString()
                                : "-1",
                        codex_approval_policy:
                            assistantDetail.model_configs.find((config) => config.name === "codex_approval_policy")
                                ?.value || "default",
                        codex_sandbox:
                            assistantDetail.model_configs.find((config) => config.name === "codex_sandbox")
                                ?.value || "default",
                        codex_approvals_reviewer:
                            assistantDetail.model_configs.find((config) => config.name === "codex_approvals_reviewer")
                                ?.value || "default",
                        ...assistantTypeCustomField.reduce((acc, field) => {
                            acc[field.key] =
                                field.value.type === "checkbox"
                                    ? assistantDetail.model_configs.find((config) => config.name === field.key)
                                        ?.value === "true"
                                    : assistantDetail.model_configs.find((config) => config.name === field.key)
                                        ?.value ?? "";
                            return acc;
                        }, {} as Record<string, any>),
                        ...pluginConfigValues,
                    });
                    if (
                        assistantDetail.assistant.assistant_type === 4
                        && (assistantDetail.model[0]?.provider_id ?? 0) > 0
                    ) {
                        try {
                            const runtime = await invoke<{ agent_kind: string }>("get_agent_runtime_info", {
                                assistantId: assistantDetail.assistant.id,
                            });
                            if (runtime.agent_kind === "codex_app_server") {
                                await loadAgentModelOptions(assistantDetail);
                            } else {
                                setAgentModelOptions([]);
                                setAgentModelError(null);
                            }
                        } catch (error) {
                            setAgentModelOptions([]);
                            setAgentModelError(`无法识别 Agent 提供商：${getErrorMessage(error)}`);
                        }
                    } else {
                        setAgentModelOptions([]);
                        setAgentModelError(null);
                    }
                    setAssistantTypeCustomField([]);
                    const plugin = assistantTypePluginMap.get(assistantDetail.assistant.assistant_type);
                    plugin?.onAssistantTypeSelect?.(assistantTypeApi);
                });
            }
        },
        [
            currentAssistant,
            assistantTypeCustomField,
            assistantTypePluginMap,
            assistantTypeApi,
            form,
            loadAssistantDetail,
            loadPluginAssistantConfigValues,
            loadAgentModelOptions,
        ]
    );

    // 监听助手列表变化
    useAssistantListListener({
        onAssistantListChanged: useCallback(
            (assistantList: AssistantListItem[]) => {
                setAssistants(assistantList);

                if (assistantList.length === 0) {
                    setCurrentAssistant(null);
                    return;
                }

                if (!currentAssistant) {
                    handleChooseAssistant(assistantList[0]);
                    return;
                }

                const currentAssistantExists = assistantList.some(
                    (assistant) => assistant.id === currentAssistant.assistant.id
                );

                if (!currentAssistantExists) {
                    handleChooseAssistant(assistantList[0]);
                }
            },
            [currentAssistant, handleChooseAssistant, setAssistants, setCurrentAssistant]
        ),
    });

    // 修改配置
    const handleConfigChange = useCallback(
        (key: string, value: string | boolean, value_type: string) => {
            console.log("handleConfigChange", key, value, value_type, currentAssistant);
            if (currentAssistant) {
                const index = currentAssistant.model_configs.findIndex((config) => config.name === key);
                const { isValid, parsedValue } = validateConfig(value, value_type);
                if (!isValid) return;

                // 更新表单值
                form.setValue(key, parsedValue);

                // 更新模型配置
                setCurrentAssistant((prev) => {
                    if (!prev) return prev;
                    const newConfigs =
                        index !== -1
                            ? prev.model_configs.map((config, i) =>
                                i === index
                                    ? {
                                        ...config,
                                        value: parsedValue.toString(),
                                    }
                                    : config
                            )
                            : [
                                ...prev.model_configs,
                                {
                                    name: key,
                                    value: parsedValue.toString(),
                                    value_type: value_type,
                                    id: 0,
                                    assistant_id: prev.assistant.id,
                                    assistant_model_id: prev.model[0]?.id ?? 0,
                                },
                            ];
                    return { ...prev, model_configs: newConfigs };
                });
            }
        },
        [currentAssistant, form, setCurrentAssistant]
    );

    // 修改 prompt
    const handlePromptChange = useCallback(
        (value: string) => {
            if (!currentAssistant?.prompts.length) return;

            setCurrentAssistant((prev) => {
                if (!prev) return prev;
                return {
                    ...prev,
                    prompts: [
                        {
                            ...prev.prompts[0],
                            prompt: value,
                        },
                    ],
                };
            });
        },
        [currentAssistant, setCurrentAssistant]
    );

    const handlePluginConfigChange = useCallback(
        (formKey: string, value: string | boolean) => {
            form.setValue(formKey, value);
            setPluginAssistantConfigValues((prev) => ({
                ...prev,
                [formKey]: value,
            }));
        },
        [form]
    );

    // 使用新的 hook 生成表单配置
    const { formConfig } = useAssistantFormConfig({
        currentAssistant,
        assistantTypeNameMap,
        assistantTypeCustomField,
        assistantTypeCustomLabel,
        assistantTypeCustomTips,
        assistantTypeHideField,
        navigateTo,
        onConfigChange: handleConfigChange,
        onPromptChange: handlePromptChange,
        pluginAssistantFormFields,
        pluginAssistantConfigValues,
        onPluginConfigChange: handlePluginConfigChange,
        agentModelOptions,
        agentModelLoading,
        agentModelError,
        onAgentProviderChange: handleAgentProviderChange,
        onAgentModelChange: applyAgentModelSelection,
    });

    // 保存助手
    const handleAssistantFormSave = useCallback(() => {
        if (!currentAssistant) return;

        const values = form.getValues();

        saveAssistant({
            ...currentAssistant,
            assistant: {
                ...currentAssistant.assistant,
                assistant_type: values.assistantType,
                name: currentAssistant.assistant.name,
                description: currentAssistant.assistant.description,
            },
            model: (() => {
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
                } else if (currentAssistant.model.length > 0) {
                    // 保留原有模型配置
                    return currentAssistant.model;
                } else {
                    // 没有模型配置，返回空数组
                    return [];
                }
            })(),
            model_configs: Object.entries(values)
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
                    const customField = assistantTypeCustomField.find((field) => field.key === key);

                    // ACP/Codex 助手专用字段（assistant_type === 4）
                    const isAcpField =
                        (key.startsWith("acp_") || key.startsWith("codex_")) &&
                        currentAssistant.assistant.assistant_type === 4;
                    if (isAcpField) {
                        return true;
                    }

                    // 内置字段（如 reasoning_effort）允许保存
                    if (key === "reasoning_effort") {
                        return true;
                    }

                    if (key === "use_native_toolcall") {
                        return true;
                    }

                    // 如果是插件自定义字段，直接允许保存
                    if (customField) {
                        return true;
                    }

                    // 原有的过滤逻辑
                    return (
                        config &&
                        config.value_type &&
                        config?.value_type !== "static" &&
                        config?.value_type !== "button" &&
                        config?.value_type !== "custom"
                    );
                })
                .map(([key, value]) => {
                    const config = currentAssistant.model_configs.find((config) => config.name === key);
                    const customField = assistantTypeCustomField.find((field) => field.key === key);

                    // 为插件自定义字段和内置字段确定正确的 value_type
                    let valueType = config?.value_type ?? "string";

                    // ACP/Codex 助手专用字段
                    const isAcpField =
                        (key.startsWith("acp_") || key.startsWith("codex_")) &&
                        currentAssistant.assistant.assistant_type === 4;
                    if (isAcpField) {
                        valueType = "string";
                    } else if (key === "use_native_toolcall") {
                        valueType = "boolean";
                    } else if (customField) {
                        // 根据插件字段的类型映射到数据库的 value_type
                        const fieldType = customField.value.type;
                        if (fieldType === "checkbox" || fieldType === "switch") {
                            valueType = "boolean";
                        } else if (fieldType === "select" || fieldType === "radio") {
                            valueType = "string";
                        } else {
                            valueType = "string";
                        }
                    } else if (key === "reasoning_effort") {
                        // 内置 reasoning_effort 字段
                        valueType = "string";
                    }

                    // Codex 字段的 "default" 表示跟随提供商配置，落库时归一化为空字符串
                    const normalizedValue = key.startsWith("codex_") && value === "default" ? "" : value;

                    return {
                        name: key,
                        value: normalizedValue != null ? normalizedValue.toString() : null,
                        value_type: valueType,
                        id: config?.id ?? 0,
                        assistant_id: currentAssistant.assistant.id,
                        assistant_model_id: currentAssistant.model[0]?.id ?? 0,
                    };
                }),
            prompts: [
                {
                    ...currentAssistant.prompts[0],
                    prompt: values.prompt,
                },
            ],
        })
            .then(async () => {
                await Promise.all(
                    pluginAssistantFormFields.map((field) =>
                        invoke("set_plugin_assistant_config", {
                            pluginId: field.pluginId,
                            assistantId: currentAssistant.assistant.id,
                            key: field.key,
                            value:
                                values[field.formKey] == null
                                    ? null
                                    : typeof values[field.formKey] === "boolean"
                                        ? String(values[field.formKey])
                                        : String(values[field.formKey]),
                        })
                    )
                );
                toast.success("保存成功");
                await loadAssistantDetail(currentAssistant.assistant.id);
                await loadPluginAssistantConfigValues(currentAssistant.assistant.id);
            })
            .catch((error) => toast.error("保存失败: " + error));
    }, [currentAssistant, form, saveAssistant, assistantTypeCustomField, loadAssistantDetail, loadPluginAssistantConfigValues, pluginAssistantFormFields]);

    // 删除助手
    const handleDelete = useCallback(() => {
        deleteAssistant()
            .then((result) => {
                if (result.shouldSelectFirst && result.assistants.length > 0) {
                    handleChooseAssistant(result.assistants[0]);
                }
                closeConfirmDeleteDialog();
            })
            .catch(() => {
                // 错误已在 hook 中处理
            });
    }, [deleteAssistant, closeConfirmDeleteDialog, handleChooseAssistant]);

    // 添加新助手处理
    const handleAssistantAdded = useCallback(
        (assistantDetail: AssistantDetail) => {
            addAssistant(assistantDetail);
            setPluginAssistantConfigValues({});
            setAgentModelOptions([]);
            setAgentModelError(null);

            // 重置表单状态为新助手的配置
            form.reset({
                assistantType: assistantDetail.assistant.assistant_type,
                model:
                    assistantDetail.model.length > 0
                        ? `${assistantDetail.model[0].model_code}%%${assistantDetail.model[0].provider_id}`
                        : "-1",
                agent_model: "",
                prompt: assistantDetail.prompts[0]?.prompt || "",
                ...assistantDetail.model_configs.reduce((acc, config) => {
                    acc[config.name] = config.value_type === "boolean" ? config.value == "true" : config.value;
                    return acc;
                }, {} as Record<string, any>),
                acp_provider:
                    assistantDetail.assistant.assistant_type === 4 &&
                    assistantDetail.model.length > 0 &&
                    assistantDetail.model[0].provider_id > 0
                        ? assistantDetail.model[0].provider_id.toString()
                        : "-1",
                codex_approval_policy: "default",
                codex_sandbox: "default",
                codex_approvals_reviewer: "default",
            });
        },
        [addAssistant, form]
    );

    // 分享助手
    const handleShareAssistant = useCallback(async () => {
        try {
            const code = await shareAssistant();
            openShareDialog(code);
        } catch (error) {
            // 错误已在 hook 中处理
        }
    }, [shareAssistant, openShareDialog]);

    // 下拉菜单选项
    const selectOptions: SelectOption[] = useMemo(
        () =>
            assistants.map((assistant) => ({
                id: assistant.id.toString(),
                label: assistant.name,
                icon: <User className="h-4 w-4" />,
            })),
        [assistants]
    );

    // 下拉菜单选择回调
    const handleSelectFromDropdown = useCallback(
        (assistantId: string) => {
            const assistant = assistants.find((a) => a.id.toString() === assistantId);
            if (assistant) {
                handleChooseAssistant(assistant);
            }
        },
        [assistants, handleChooseAssistant]
    );

    // 新增按钮组件
    const addButton = useMemo(
        () => (
            <div className="flex gap-2">
                <AddAssistantDialog
                    assistantTypes={assistantTypes}
                    onAssistantAdded={handleAssistantAdded}
                    triggerButtonProps={{
                        className:
                            "gap-2 bg-primary hover:bg-primary/90 text-primary-foreground shadow-sm hover:shadow-md transition-all",
                    }}
                />
                <Tooltip delayDuration={500}>
                    <TooltipTrigger asChild>
                        <Button
                            variant="outline"
                            onClick={openImportDialog}
                            className="shadow-sm hover:shadow-md transition-all"
                        >
                            <Download className="h-4 w-4" />
                        </Button>
                    </TooltipTrigger>
                    <TooltipContent>导入助手</TooltipContent>
                </Tooltip>
            </div>
        ),
        [assistantTypes, handleAssistantAdded, openImportDialog]
    );

    // 按搜索词过滤助手（支持拼音）
    const filteredAssistants = useMemo(() => {
        if (!searchQuery.trim()) return assistants;
        return assistants.filter(assistant =>
            PinyinFilter.matches(assistant.name, searchQuery)
        );
    }, [assistants, searchQuery, pinyinReady]);

    // 侧边栏内容 - 使用 useMemo 避免重复创建
    const sidebar = useMemo(() => (
        <SidebarList title="助手列表" description="选择助手进行配置" icon={<Bot className="h-5 w-5" />} addButton={addButton}
            searchValue={searchQuery} onSearchChange={setSearchQuery} searchPlaceholder="搜索助手...">
            {filteredAssistants.map((assistant) => (
                <ListItemButton
                    key={assistant.id}
                    isSelected={currentAssistant?.assistant.id === assistant.id}
                    onClick={() => handleChooseAssistant(assistant)}
                >
                    <span className="font-medium truncate">{assistant.name}</span>
                </ListItemButton>
            ))}
        </SidebarList>
    ), [filteredAssistants, currentAssistant?.assistant.id, handleChooseAssistant, addButton, searchQuery]);

    // 右侧内容 - 使用 useMemo 避免重复创建（必须在条件返回之前）
    const content = useMemo(() => currentAssistant ? (
        <AssistantFormRenderer
            currentAssistant={currentAssistant}
            formConfig={formConfig}
            form={form}
            assistantConfigApi={assistantConfigApi}
            onSave={handleAssistantFormSave}
            onCopy={currentAssistant.assistant.id === 1 ? undefined : copyAssistant}
            onDelete={currentAssistant.assistant.id === 1 ? undefined : openConfirmDeleteDialog}
            onEdit={openUpdateFormDialog}
            onShare={handleShareAssistant}
        />
    ) : (
        <EmptyState
            icon={<Settings className="h-8 w-8 text-muted-foreground" />}
            title="选择一个助手"
            description="从左侧列表中选择一个助手开始配置"
        />
    ), [currentAssistant, formConfig, form, assistantConfigApi, handleAssistantFormSave, copyAssistant, openConfirmDeleteDialog, openUpdateFormDialog, handleShareAssistant]);

    // 空状态
    if (assistants.length === 0) {
        return (
            <>
                <ConfigPageLayout
                    sidebar={null}
                    content={
                        <EmptyState
                            icon={<Bot className="h-8 w-8 text-muted-foreground" />}
                            title="还没有配置助手"
                            description="创建你的第一个AI助手，开始享受个性化的智能对话体验"
                            action={
                                <div className="flex flex-col gap-3">
                                    <div className="flex gap-2 justify-center">
                                        <AddAssistantDialog
                                            assistantTypes={assistantTypes}
                                            onAssistantAdded={handleAssistantAdded}
                                        />
                                        <Tooltip delayDuration={500}>
                                            <TooltipTrigger asChild>
                                                <Button
                                                    variant="outline"
                                                    onClick={openImportDialog}
                                                    className="shadow-lg hover:shadow-xl transition-all"
                                                >
                                                    <Download className="h-4 w-4" />
                                                </Button>
                                            </TooltipTrigger>
                                            <TooltipContent>导入助手</TooltipContent>
                                        </Tooltip>
                                    </div>
                                </div>
                            }
                        />
                    }
                />

                <AssistantDialogs
                    dialogStates={dialogStates}
                    shareCode={shareCode}
                    currentAssistant={currentAssistant}
                    onConfirmDelete={handleDelete}
                    onCancelDelete={closeConfirmDeleteDialog}
                    onSave={saveAssistant}
                    onAssistantUpdated={updateAssistantInfo}
                    onImportAssistant={importAssistant}
                    onCloseUpdateForm={closeUpdateFormDialog}
                    onCloseShare={closeShareDialog}
                    onCloseImport={closeImportDialog}
                />
            </>
        );
    }

    return (
        <>
            <ConfigPageLayout
                sidebar={sidebar}
                content={content}
                selectOptions={selectOptions}
                selectedOptionId={currentAssistant?.assistant.id.toString()}
                onSelectOption={handleSelectFromDropdown}
                selectPlaceholder="选择助手"
                addButton={addButton}
            />

            <AssistantDialogs
                dialogStates={dialogStates}
                shareCode={shareCode}
                currentAssistant={currentAssistant}
                onConfirmDelete={handleDelete}
                onCancelDelete={closeConfirmDeleteDialog}
                onSave={saveAssistant}
                onAssistantUpdated={updateAssistantInfo}
                onImportAssistant={importAssistant}
                onCloseUpdateForm={closeUpdateFormDialog}
                onCloseShare={closeShareDialog}
                onCloseImport={closeImportDialog}
            />
        </>
    );
};

export default AssistantConfig;
