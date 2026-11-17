import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import type { AssistantDetail } from "@/data/Assistant";
import type { LoadedPlugin } from "@/services/PluginRuntime";
import type { AssistantConfigApi, AssistantFormConfig } from "@/types/forms";
import { useAssistantTypePlugin } from "@/hooks/assistant/useAssistantTypePlugin";
import { useAssistantFormConfig } from "@/hooks/assistant/useAssistantFormConfig";
import { ASSISTANT_ADVANCED_FIELD_KEYS } from "@/utils/assistantFormSections";
import { resolveAgentDefaultSelection, type AgentModelOption } from "@/utils/agentDefaults";
import { validateConfig } from "@/utils/validate";
import { getErrorMessage } from "@/utils/error";
import { isAssistantFormDirty } from "./assistantFormDirty";
import {
    AGENT_MODEL_OPTION_KINDS,
    buildAssistantFormValues,
    buildAssistantSaveDetail,
    collectPluginAssistantFormFields,
    fetchPluginAssistantConfigValues,
    savePluginAssistantConfigValues,
} from "./assistantEditorModel";

const NOOP = () => {};

export interface AssistantSetupEditor {
    form: ReturnType<typeof useForm>;
    /** 表单值已按助手回填，可以展示字段。 */
    ready: boolean;
    loadError: string | null;
    advancedFields: AssistantFormConfig[];
    extensionFields: AssistantFormConfig[];
    assistantConfigApi: AssistantConfigApi;
    /**
     * 有改动时按编辑页同样的规则保存，返回保存后的助手详情；没有改动返回 null。
     * `profile` 是第一步填写的名称、描述与提示词（回到上一步修改过时以此为准；不传提示词则保持不变）。
     */
    save: (profile: { name: string; description: string; prompt?: string }) => Promise<AssistantDetail | null>;
}

/**
 * 新增助手向导「高级与扩展」步骤的编辑器：对一个刚创建的助手复用编辑页的字段生成
 * （useAssistantFormConfig）与保存规则（assistantEditorModel），只取高级配置与扩展配置两组字段。
 * 普通对话助手与智能体助手的字段差异全部由 useAssistantFormConfig 按类型决定。
 */
export function useAssistantSetupEditor(
    detail: AssistantDetail | null,
    pluginList: LoadedPlugin[],
    navigateTo: (menuKey: string) => void,
): AssistantSetupEditor {
    const form = useForm();
    const [assistant, setAssistant] = useState<AssistantDetail | null>(null);
    const [readyId, setReadyId] = useState<number | null>(null);
    const [loadError, setLoadError] = useState<string | null>(null);
    const [pluginValues, setPluginValues] = useState<Record<string, string | boolean>>({});
    const [agentModelOptions, setAgentModelOptions] = useState<AgentModelOption[]>([]);
    const [agentModelLoading, setAgentModelLoading] = useState(false);
    const [agentModelError, setAgentModelError] = useState<string | null>(null);
    const baselineRef = useRef<Record<string, unknown>>({});

    const {
        assistantTypeNameMap,
        assistantTypePluginMap,
        assistantTypeCustomField,
        setAssistantTypeCustomField,
        assistantTypeCustomLabel,
        assistantTypeCustomTips,
        assistantTypeHideField,
        assistantTypeApi,
    } = useAssistantTypePlugin(pluginList);
    const pluginFields = useMemo(() => collectPluginAssistantFormFields(pluginList), [pluginList]);

    const applyAgentModelSelection = useCallback((modelValue: string, effort: string) => {
        const [modelCode, providerValue] = modelValue.split("%%");
        const providerId = Number.parseInt(providerValue, 10);
        form.setValue("agent_model", modelValue);
        form.setValue("reasoning_effort", effort);
        setAssistant((previous) => {
            if (!previous || !Number.isFinite(providerId) || providerId <= 0) return previous;
            const existingModel = previous.model[0];
            const nextModel = {
                id: existingModel?.id ?? 0,
                assistant_id: existingModel?.assistant_id ?? previous.assistant.id,
                model_code: modelCode,
                provider_id: providerId,
                alias: existingModel?.alias ?? "",
            };
            const others = previous.model_configs.filter((config) => config.name !== "reasoning_effort");
            const existing = previous.model_configs.find((config) => config.name === "reasoning_effort");
            return {
                ...previous,
                model: [nextModel],
                model_configs: [...others, {
                    id: existing?.id ?? 0,
                    assistant_id: previous.assistant.id,
                    assistant_model_id: existingModel?.id ?? 0,
                    name: "reasoning_effort",
                    value: effort,
                    value_type: "string",
                }],
            };
        });
    }, [form]);

    // 载入（或换成另一个草稿）：与编辑页选择助手的流程一致
    const detailId = detail?.assistant.id ?? null;
    useEffect(() => {
        if (!detail) {
            setAssistant(null);
            setReadyId(null);
            return;
        }
        let active = true;
        setAssistant(detail);
        setReadyId(null);
        setLoadError(null);
        setAgentModelOptions([]);
        setAgentModelError(null);
        setAssistantTypeCustomField([]);
        assistantTypePluginMap.get(detail.assistant.assistant_type)?.onAssistantTypeSelect?.(assistantTypeApi);
        void (async () => {
            try {
                const values = await fetchPluginAssistantConfigValues(pluginFields, detail.assistant.id);
                if (!active) return;
                setPluginValues(values);
                form.reset(buildAssistantFormValues(detail, [], values));
                baselineRef.current = structuredClone(form.getValues());
                setReadyId(detail.assistant.id);
            } catch (error) {
                if (active) setLoadError(`读取助手 ${detail.assistant.id} 的插件配置失败：${getErrorMessage(error)}`);
                return;
            }
            if (detail.assistant.assistant_type !== 4 || (detail.model[0]?.provider_id ?? 0) <= 0) return;
            // 智能体助手：读取默认模型与思考强度选项（Codex 的思考强度选项依赖它）
            try {
                const runtime = await invoke<{ agent_kind: string }>("get_agent_runtime_info", { assistantId: detail.assistant.id });
                if (!active || !AGENT_MODEL_OPTION_KINDS.includes(runtime.agent_kind)) return;
                setAgentModelLoading(true);
                const options = await invoke<AgentModelOption[]>("get_agent_model_options", { assistantId: detail.assistant.id, providerId: null });
                if (!active) return;
                setAgentModelOptions(options);
                const selection = resolveAgentDefaultSelection(
                    options,
                    detail.model[0]?.model_code,
                    detail.model_configs.find((config) => config.name === "reasoning_effort")?.value,
                );
                if (selection.model) {
                    applyAgentModelSelection(selection.model.code, selection.effort);
                    // 自动解析出的默认值属于已加载状态，不算用户修改
                    baselineRef.current = {
                        ...baselineRef.current,
                        agent_model: selection.model.code,
                        reasoning_effort: selection.effort,
                    };
                }
            } catch (error) {
                if (active) setAgentModelError(`无法读取 Agent 默认配置：${getErrorMessage(error)}`);
            } finally {
                if (active) setAgentModelLoading(false);
            }
        })();
        return () => { active = false; };
        // 只在换了草稿时重新载入；同一草稿的名称变化在保存时处理
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [detailId]);

    // 助手类型插件在选择后才注册自定义字段：补上表单值与基线，未改动时不算脏
    useEffect(() => {
        if (!assistant || readyId !== assistant.assistant.id) return;
        const values = form.getValues();
        for (const field of assistantTypeCustomField) {
            if (field.key in values) continue;
            const config = assistant.model_configs.find((item) => item.name === field.key);
            const value = field.value.type === "checkbox" ? config?.value === "true" : config?.value ?? "";
            form.setValue(field.key, value);
            baselineRef.current = { ...baselineRef.current, [field.key]: value };
        }
    }, [assistant, assistantTypeCustomField, form, readyId]);

    const handleConfigChange = useCallback((key: string, value: string | boolean, valueType: string) => {
        const { isValid, parsedValue } = validateConfig(value, valueType);
        if (!isValid) return;
        form.setValue(key, parsedValue);
        setAssistant((previous) => {
            if (!previous) return previous;
            const index = previous.model_configs.findIndex((config) => config.name === key);
            const model_configs = index !== -1
                ? previous.model_configs.map((config, i) => i === index ? { ...config, value: parsedValue.toString() } : config)
                : [...previous.model_configs, {
                    name: key,
                    value: parsedValue.toString(),
                    value_type: valueType,
                    id: 0,
                    assistant_id: previous.assistant.id,
                    assistant_model_id: previous.model[0]?.id ?? 0,
                }];
            return { ...previous, model_configs };
        });
    }, [form]);

    const handlePluginConfigChange = useCallback((formKey: string, value: string | boolean) => {
        form.setValue(formKey, value);
        setPluginValues((previous) => ({ ...previous, [formKey]: value }));
    }, [form]);

    const assistantConfigApi: AssistantConfigApi = useMemo(() => ({
        clearFieldValue: (fieldName: string) => handleConfigChange(fieldName, "", ""),
        changeFieldValue: (fieldName: string, value: any, valueType: string) => handleConfigChange(fieldName, value, valueType),
    }), [handleConfigChange]);

    const { formConfig } = useAssistantFormConfig({
        currentAssistant: assistant,
        assistantTypeNameMap,
        assistantTypeCustomField,
        assistantTypeCustomLabel,
        assistantTypeCustomTips,
        assistantTypeHideField,
        navigateTo,
        onConfigChange: handleConfigChange,
        // 提示词、提供商与默认模型不在这一步
        onPromptChange: NOOP,
        pluginAssistantFormFields: pluginFields,
        pluginAssistantConfigValues: pluginValues,
        onPluginConfigChange: handlePluginConfigChange,
        agentModelOptions,
        agentModelLoading,
        agentModelError,
        onAgentProviderChange: NOOP,
        onAgentModelChange: applyAgentModelSelection,
    });

    const { advancedFields, extensionFields } = useMemo(() => ({
        advancedFields: formConfig.filter((item) => item.section !== "extension" && ASSISTANT_ADVANCED_FIELD_KEYS.includes(item.key)),
        extensionFields: formConfig.filter((item) => item.section === "extension"),
    }), [formConfig]);

    const save = useCallback(async ({ prompt, ...profile }: { name: string; description: string; prompt?: string }) => {
        if (!assistant) return null;
        const values = structuredClone(form.getValues());
        const promptChanged = prompt !== undefined && prompt !== (assistant.prompts[0]?.prompt ?? "");
        const profileChanged = profile.name !== assistant.assistant.name
            || profile.description !== (assistant.assistant.description ?? "");
        if (!profileChanged && !promptChanged && !isAssistantFormDirty(values, baselineRef.current)) return null;
        const id = assistant.assistant.id;
        const next = buildAssistantSaveDetail(
            { ...assistant, assistant: { ...assistant.assistant, ...profile } },
            prompt === undefined ? values : { ...values, prompt },
            assistantTypeCustomField,
        );
        await invoke("save_assistant", { assistantDetail: next });
        // 换成库里的最新详情：新增配置拿到 id，失败重试时不会重复插入
        const saved = await invoke<AssistantDetail>("get_assistant", { assistantId: id });
        setAssistant(saved);
        await savePluginAssistantConfigValues(pluginFields, id, values);
        baselineRef.current = values;
        return saved;
    }, [assistant, assistantTypeCustomField, form, pluginFields]);

    return {
        form,
        ready: assistant !== null && readyId === assistant.assistant.id,
        loadError,
        advancedFields,
        extensionFields,
        assistantConfigApi,
        save,
    };
}
