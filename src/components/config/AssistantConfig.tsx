import AssistantItemContextMenu, { type AssistantItemAction } from "./assistant/AssistantItemContextMenu";
import { useAssistantItemActions } from "@/hooks/assistant/useAssistantItemActions";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { AssistantDetail, AssistantListItem } from "../../data/Assistant";
import { Bot, Settings, User, Download } from "lucide-react";
import { Button } from "../ui/button";
import { Tooltip, TooltipTrigger, TooltipContent } from "../ui/tooltip";
import { useForm } from "react-hook-form";
import { validateConfig } from "../../utils/validate";
import { PinyinFilter, usePinyinReady } from "../../utils/pinyinFilter";
import AddAssistantDialog from "./AddAssistantDialog";

// 导入公共组件
import { ConfigPageLayout, SidebarList, ListItemButton, EmptyState, ConfigPageLoading, SelectOption } from "../common";

// 导入新的 hooks 和组件
import { useAssistantTypePlugin } from "@/hooks/assistant/useAssistantTypePlugin";
import { useAssistantOperations } from "@/hooks/assistant/useAssistantOperations";
import { useAssistantFormConfig } from "@/hooks/assistant/useAssistantFormConfig";
import { useDialogStates } from "@/hooks/assistant/useDialogStates";
import { AssistantFormRenderer } from "./assistant/AssistantFormRenderer";
import { AssistantDialogs } from "./assistant/AssistantDialogs";
import { AssistantConfigApi } from "@/types/forms";
import type { LoadedPlugin } from "@/services/PluginRuntime";
import { getErrorMessage } from "@/utils/error";
import { registerConfigBeforeLeave } from "@/lib/configLeaveGuard";
import { isAssistantFormDirty } from "./assistant/assistantFormDirty";
import {
    AGENT_MODEL_OPTION_KINDS,
    buildAssistantFormValues,
    buildAssistantSaveDetail,
    collectPluginAssistantFormFields,
    fetchPluginAssistantConfigValues,
    savePluginAssistantConfigValues,
} from "./assistant/assistantEditorModel";
import {
    AlertDialog,
    AlertDialogContent,
    AlertDialogDescription,
    AlertDialogFooter,
    AlertDialogHeader,
    AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { resolveAgentDefaultSelection } from "@/utils/agentDefaults";
import type { AgentModelOption } from "@/utils/agentDefaults";

interface AssistantConfigProps {
    pluginList: LoadedPlugin[];
    navigateTo: (menuKey: string) => void;
}

/** Clone form values so later edits cannot mutate the discard snapshot. */
const cloneFormSnapshot = <T extends Record<string, any>>(values: T): T =>
    structuredClone(values);

const AssistantConfig: React.FC<AssistantConfigProps> = ({ pluginList, navigateTo }) => {
    const form = useForm();
    const [leavePrompt, setLeavePromptState] = useState<null | { proceed: () => void }>(null);
    const leavePromptRef = useRef<typeof leavePrompt>(null);
    const setLeavePrompt = useCallback((prompt: typeof leavePrompt) => {
        // Update immediately so a canceled/replaced prompt cannot navigate when
        // an older save resolves before the next render.
        leavePromptRef.current = prompt;
        setLeavePromptState(prompt);
    }, []);
    useEffect(() => () => { leavePromptRef.current = null; }, []);
    /** Last loaded/saved form values — discard must restore this, not empty useForm defaults. */
    const loadedFormSnapshotRef = useRef<Record<string, unknown>>({});
    // Dirty = current values differ from the loaded/saved baseline (not RHF isDirty, see assistantFormDirty).
    // Guards evaluate it on demand; the badge state follows value changes asynchronously.
    const isFormDirtyNow = useCallback(
        () => isAssistantFormDirty(form.getValues(), loadedFormSnapshotRef.current),
        [form]
    );
    const [isDirty, setIsDirty] = useState(false);
    // Do not mount a new assistant's Controllers against the previous assistant's values.
    const [formAssistantId, setFormAssistantId] = useState<number | null>(null);
    const [isSaving, setIsSaving] = useState(false);
    const saveInFlightRef = useRef<Promise<void> | null>(null);
    const dirtyCheckQueuedRef = useRef(false);
    const scheduleDirtyCheck = useCallback(() => {
        if (dirtyCheckQueuedRef.current) return;
        dirtyCheckQueuedRef.current = true;
        // Field registration can emit during render; never setState synchronously from it.
        queueMicrotask(() => {
            dirtyCheckQueuedRef.current = false;
            setIsDirty(isFormDirtyNow());
        });
    }, [isFormDirtyNow]);
    useEffect(
        () => form.subscribe({ formState: { values: true }, callback: scheduleDirtyCheck }),
        [form, scheduleDirtyCheck]
    );
    /** Values just loaded or saved become the clean baseline. */
    const commitFormBaseline = useCallback((values: Record<string, unknown>) => {
        loadedFormSnapshotRef.current = cloneFormSnapshot(values);
        scheduleDirtyCheck();
    }, [scheduleDirtyCheck]);
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

    const pluginAssistantFormFields = useMemo(() => collectPluginAssistantFormFields(pluginList), [pluginList]);

    const loadPluginAssistantConfigValues = useCallback(
        async (assistantId: number, isCurrent: () => boolean = () => true) => {
            const nextValues = await fetchPluginAssistantConfigValues(pluginAssistantFormFields, assistantId);
            if (isCurrent()) setPluginAssistantConfigValues(nextValues);
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
        async (assistantDetail: AssistantDetail, providerId: number | null = null, isCurrent: () => boolean = () => true) => {
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
                if (!isCurrent()) return;
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
                    // 加载时自动解析的默认模型/推理强度属于已加载状态，不是用户编辑
                    if (providerId == null) {
                        commitFormBaseline({
                            ...loadedFormSnapshotRef.current,
                            agent_model: selection.model.code,
                            reasoning_effort: selection.effort,
                        });
                    }
                }
            } catch (error) {
                if (!isCurrent()) return;
                const message = `无法读取 Agent 默认配置：${getErrorMessage(error)}`;
                setAgentModelOptions([]);
                setAgentModelError(message);
                toast.error(message);
            } finally {
                if (isCurrent()) setAgentModelLoading(false);
            }
        },
        [applyAgentModelSelection, commitFormBaseline]
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
                ["codex_app_server", "pi", "cursor", "opencode", "kiro", "antigravity"].includes(apiType ?? "")
                && currentAssistant
                && Number.isFinite(providerId)
                && providerId > 0
            ) {
                void loadAgentModelOptions(currentAssistant, providerId);
            }
        },
        [currentAssistant, form, loadAgentModelOptions, setCurrentAssistant]
    );

    // 首次进入：列表与首个助手详情（含表单回填）都完成后才展示，避免先闪空状态/空表单
    const [initialLoaded, setInitialLoaded] = useState(false);

    // 初始化助手列表
    useEffect(() => {
        loadAssistants()
            .then((assistantList) => {
                if (assistantList.length) {
                    return handleChooseAssistant(assistantList[0]);
                }
            })
            // 列表/详情读取失败已在 useAssistantOperations 中 toast 具体原因
            .catch(() => undefined)
            .finally(() => setInitialLoaded(true));
    }, [loadAssistants]);
    // Only explicit selections invalidate a refresh. Its own fallback selection
    // must finish loading plugin values and resetting the form.
    const [selectionRevision, setSelectionRevision] = useState(0);
    const selectionRequestRef = useRef(0);
    const invalidateSelection = useCallback(() => {
        selectionRequestRef.current += 1;
        setSelectionRevision((revision) => revision + 1);
    }, []);
    // 选择助手
    const handleChooseAssistant = useCallback(
        (assistant: AssistantListItem, force = false, isCurrent: () => boolean = () => true) => {
            if (force || !currentAssistant || currentAssistant.assistant.id !== assistant.id) {
                const request = ++selectionRequestRef.current;
                if (!force) setSelectionRevision((revision) => revision + 1);
                const isSelectionCurrent = () => request === selectionRequestRef.current && isCurrent();
                return loadAssistantDetail(assistant.id, isSelectionCurrent).then(async (assistantDetail) => {
                    if (!isSelectionCurrent()) return;
                    const pluginConfigValues = await loadPluginAssistantConfigValues(
                        assistantDetail.assistant.id, isSelectionCurrent
                    );
                    if (!isSelectionCurrent()) return;
                    form.reset(buildAssistantFormValues(assistantDetail, assistantTypeCustomField, pluginConfigValues));
                    commitFormBaseline(form.getValues());
                    setFormAssistantId(assistantDetail.assistant.id);
                    if (
                        assistantDetail.assistant.assistant_type === 4
                        && (assistantDetail.model[0]?.provider_id ?? 0) > 0
                    ) {
                        try {
                            const runtime = await invoke<{ agent_kind: string }>("get_agent_runtime_info", {
                                assistantId: assistantDetail.assistant.id,
                            });
                            if (!isSelectionCurrent()) return;
                            if (AGENT_MODEL_OPTION_KINDS.includes(runtime.agent_kind)) {
                                await loadAgentModelOptions(assistantDetail, null, isSelectionCurrent);
                            } else {
                                setAgentModelOptions([]);
                                setAgentModelError(null);
                            }
                        } catch (error) {
                            if (!isSelectionCurrent()) return;
                            setAgentModelOptions([]);
                            setAgentModelError(`无法识别 Agent 提供商：${getErrorMessage(error)}`);
                        }
                    } else {
                        setAgentModelOptions([]);
                        setAgentModelError(null);
                    }
                    if (!isSelectionCurrent()) return;
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
            commitFormBaseline,
        ]
    );


    useEffect(() => {
        return registerConfigBeforeLeave((proceed) => {
            if (!isFormDirtyNow()) {
                proceed();
                return;
            }
            setLeavePrompt({ proceed });
        });
    }, [isFormDirtyNow]);

    useDataRefresh({
        events: ["assistant_list_changed", "llm_provider_changed", "mcp_state_changed"],
        identity: selectionRevision,
        refresh: async (isCurrent) => {
            const list = await loadAssistants(isCurrent);
            if (!isCurrent()) return;
            if (isFormDirtyNow() && currentAssistant) {
                toast.warning("助手配置已更新，未保存的编辑已保留；保存或切换助手后可重新加载。");
                return;
            }
            const selected = list.find((a) => a.id === currentAssistant?.assistant.id) ?? list[0];
            if (selected) await handleChooseAssistant(selected, true, () => isCurrent() && !isFormDirtyNow());
            else {
                selectionRequestRef.current += 1;
                setCurrentAssistant(null);
                setPluginAssistantConfigValues({});
                form.reset({});
                setFormAssistantId(null);
                commitFormBaseline({});
            }
        },
        onError: (error) => toast.error(`同步后刷新助手配置失败: ${getErrorMessage(error)}`),
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
            form.setValue("prompt", value, { shouldDirty: true });

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
        [currentAssistant, form, setCurrentAssistant]
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

    // 保存助手 — returns a Promise so leave-save can await before proceed
    const performAssistantFormSave = useCallback(async () => {
        if (!currentAssistant) return;

        const values = cloneFormSnapshot(form.getValues());
        const selection = selectionRequestRef.current;
        const isSameSelection = () => selection === selectionRequestRef.current;
        const canRefreshSavedValues = () => isSameSelection() && !isAssistantFormDirty(form.getValues(), values);

        try {
            await saveAssistant(buildAssistantSaveDetail(currentAssistant, values, assistantTypeCustomField));
            await savePluginAssistantConfigValues(pluginAssistantFormFields, currentAssistant.assistant.id, values);
            toast.success("保存成功");
            // Only the submitted values were persisted. Later edits stay dirty and must
            // not be replaced by a delayed detail/plugin refresh.
            if (isSameSelection()) commitFormBaseline(values);
            if (canRefreshSavedValues()) {
                await loadAssistantDetail(currentAssistant.assistant.id, canRefreshSavedValues);
                await loadPluginAssistantConfigValues(currentAssistant.assistant.id, canRefreshSavedValues);
            }
        } catch (error) {
            toast.error("保存失败: " + error);
            throw error;
        }
    }, [currentAssistant, form, saveAssistant, assistantTypeCustomField, loadAssistantDetail, loadPluginAssistantConfigValues, pluginAssistantFormFields, commitFormBaseline]);

    // Every save entry point shares one in-flight operation, including leave-save.
    const handleAssistantFormSave = useCallback((): Promise<void> => {
        if (saveInFlightRef.current) return saveInFlightRef.current;
        setIsSaving(true);
        const pending = performAssistantFormSave().finally(() => {
            saveInFlightRef.current = null;
            setIsSaving(false);
        });
        saveInFlightRef.current = pending;
        return pending;
    }, [performAssistantFormSave]);

    // 删除助手
    const handleDelete = useCallback(() => {
        invalidateSelection();
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
    }, [deleteAssistant, closeConfirmDeleteDialog, handleChooseAssistant, invalidateSelection]);

    const handleCopyAssistant = useCallback(async () => {
        invalidateSelection();
        const copied = await copyAssistant();
        invalidateSelection();
        await handleChooseAssistant(copied.assistant, true);
    }, [copyAssistant, handleChooseAssistant, invalidateSelection]);

    // 添加新助手处理：向导可能已保存高级/扩展配置，按正常选择流程加载助手详情与插件配置
    const handleAssistantAdded = useCallback(
        (assistantDetail: AssistantDetail) => {
            invalidateSelection();
            addAssistant(assistantDetail);
            void handleChooseAssistant(assistantDetail.assistant, true);
        },
        [addAssistant, invalidateSelection, handleChooseAssistant]
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
            if (!assistant) return;
            if (isFormDirtyNow() && currentAssistant && currentAssistant.assistant.id !== assistant.id) {
                setLeavePrompt({ proceed: () => { void handleChooseAssistant(assistant); } });
                return;
            }
            void handleChooseAssistant(assistant);
        },
        [assistants, handleChooseAssistant, currentAssistant, isFormDirtyNow]
    );

    const handleLeaveSave = useCallback(async () => {
        const prompt = leavePromptRef.current;
        if (!prompt) return;
        try {
            await handleAssistantFormSave();
            // Cancellation or a newer destination supersedes this leave request.
            if (leavePromptRef.current !== prompt || isFormDirtyNow()) return;
            setLeavePrompt(null);
            prompt.proceed();
        } catch {
            // Stay on the page with the dialog open; toast already shown by save.
        }
    }, [handleAssistantFormSave, isFormDirtyNow, setLeavePrompt]);

    const handleLeaveDiscard = useCallback(() => {
        const proceed = leavePrompt?.proceed;
        setLeavePrompt(null);
        // Fresh clone: RHF can mutate the values object passed into reset().
        form.reset(cloneFormSnapshot(loadedFormSnapshotRef.current));
        scheduleDirtyCheck();
        proceed?.();
    }, [leavePrompt, form, scheduleDirtyCheck]);

    // 新增按钮组件
    const addButton = useMemo(
        () => (
            <div className="flex gap-2">
                <AddAssistantDialog
                    assistantTypes={assistantTypes}
                    onAssistantAdded={handleAssistantAdded}
                    pluginList={pluginList}
                    navigateTo={navigateTo}
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
        [assistantTypes, handleAssistantAdded, openImportDialog, pluginList, navigateTo]
    );

    // 按搜索词过滤助手（支持拼音）
    const filteredAssistants = useMemo(() => {
        if (!searchQuery.trim()) return assistants;
        return assistants.filter(assistant =>
            PinyinFilter.matches(assistant.name, searchQuery)
        );
    }, [assistants, searchQuery, pinyinReady]);

    const itemActions = useAssistantItemActions({
        onCopied: async detail => {
            invalidateSelection();
            await loadAssistants();
            await handleChooseAssistant(detail.assistant, true);
        },
    });
    const handleItemAction = (assistant: AssistantListItem, action: AssistantItemAction) => {
        const proceed = () => { void itemActions.run(assistant, action); };
        if (isFormDirtyNow()) { setLeavePrompt({ proceed }); return; }
        proceed();
    };

    // 侧边栏内容 - 使用 useMemo 避免重复创建
    const sidebar = useMemo(() => (
        <SidebarList title="助手列表" description="选择助手进行配置" icon={<Bot className="h-5 w-5" />} addButton={addButton}
            searchValue={searchQuery} onSearchChange={setSearchQuery} searchPlaceholder="搜索助手...">
            {filteredAssistants.map((assistant) => (
                <AssistantItemContextMenu key={assistant.id} assistant={assistant} onAction={handleItemAction}>
                <ListItemButton
                    isSelected={currentAssistant?.assistant.id === assistant.id}
                    onClick={() => {
                        if (isFormDirtyNow() && currentAssistant && currentAssistant.assistant.id !== assistant.id) {
                            setLeavePrompt({ proceed: () => { void handleChooseAssistant(assistant); } });
                            return;
                        }
                        void handleChooseAssistant(assistant);
                    }}
                >
                    <span className="font-medium truncate">{assistant.name}</span>
                </ListItemButton>
                </AssistantItemContextMenu>
            ))}
        </SidebarList>
    ), [filteredAssistants, currentAssistant?.assistant.id, handleChooseAssistant, addButton, searchQuery, isFormDirtyNow, handleItemAction]);

    // 右侧内容 - 使用 useMemo 避免重复创建（必须在条件返回之前）
    const content = useMemo(() => currentAssistant ? (
        <AssistantFormRenderer
            currentAssistant={currentAssistant}
            formConfig={formConfig}
            form={form}
            isDirty={isDirty}
            isSaving={isSaving}
            isLoading={formAssistantId !== currentAssistant.assistant.id}
            assistantConfigApi={assistantConfigApi}
            onSave={() => { void handleAssistantFormSave().catch(() => undefined); }}
            onCopy={currentAssistant.assistant.id === 1 ? undefined : handleCopyAssistant}
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
    ), [currentAssistant, formConfig, form, isDirty, isSaving, formAssistantId, assistantConfigApi, handleAssistantFormSave, handleCopyAssistant, openConfirmDeleteDialog, openUpdateFormDialog, handleShareAssistant]);

    const leaveDialog = (
        <>{itemActions.dialogs}<AlertDialog open={!!leavePrompt} onOpenChange={(open) => { if (!open) setLeavePrompt(null); }}>
            <AlertDialogContent data-aipp-slot="assistant-leave-dialog">
                <AlertDialogHeader>
                    <AlertDialogTitle>有未保存的助手配置</AlertDialogTitle>
                    <AlertDialogDescription>
                        离开当前页或切换助手前，请选择保存或放弃更改。未保存时界面上的模型选择不会写入实际请求。
                    </AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <Button type="button" variant="outline" onClick={() => setLeavePrompt(null)}>取消</Button>
                    <Button type="button" variant="secondary" disabled={isSaving} onClick={handleLeaveDiscard}>放弃</Button>
                    <Button type="button" disabled={isSaving} onClick={() => { void handleLeaveSave(); }}>{isSaving ? "保存中…" : "保存"}</Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog></>
    );

    if (!initialLoaded) {
        return <ConfigPageLoading />;
    }

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
                                            pluginList={pluginList}
                                            navigateTo={navigateTo}
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
                {leaveDialog}
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

            {leaveDialog}
        </>
    );
};

export default AssistantConfig;
