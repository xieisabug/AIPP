import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { Controller } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";
import { useModels } from "@/hooks/useModels";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Form, FormControl, FormDescription, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { FolderPicker } from "@/components/config/FolderPicker";
import { ButlerOnboardingWizard } from "@/components/butler/ButlerOnboardingWizard";
import {
    buildButlerWorkspaceConfig,
    BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
    serializeButlerWorkspaceConfig,
    type TrustedWorkspace,
} from "@/components/butler/butlerWorkspaceConfig";
import { AlertTriangle, Plus, Trash2, Wand2 } from "lucide-react";

interface ExperimentalConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
    scope?: "all" | "butler";
    /** When provided, enables the onboarding wizard button inside the butler section. */
    saveFeatureConfig?: (featureCode: string, config: Record<string, unknown>) => Promise<unknown>;
    /** Called after the onboarding wizard saves config so the parent can refresh. */
    onConfigRefresh?: () => void;
}

interface MCPSummaryProgressPayload {
    phase: "started" | "processing" | "progress" | "completed";
    total: number;
    completed: number;
    succeeded: number;
    failed: number;
    server_name?: string;
    message?: string;
}

interface AssistantSummaryProgressPayload {
    phase: "started" | "processing" | "progress" | "completed";
    total: number;
    completed: number;
    succeeded: number;
    failed: number;
    assistant_name?: string;
    message?: string;
}

interface FeishuRuntimeStatus {
    butler_enabled: boolean;
    enabled: boolean;
    configured: boolean;
    secret_configured: boolean;
    running: boolean;
    connected: boolean;
    app_id?: string | null;
    base_url?: string | null;
    allow_p2p: boolean;
    allow_group: boolean;
    group_require_mention: boolean;
    last_error?: string | null;
    last_event_at?: string | null;
    last_status_at?: string | null;
    status_detail?: string | null;
    status_text: string;
}

interface ExperimentalSummaryTaskStatus {
    mcp_running: boolean;
    assistant_running: boolean;
    conversation_running: boolean;
    conversation_running_count: number;
}

interface SummaryTriggerResult {
    started: boolean;
    already_running: boolean;
    message: string;
}

type SummaryTaskKey = "mcp" | "assistant" | "conversation";

interface SummaryEnabledState {
    mcp: boolean;
    assistant: boolean;
    conversation: boolean;
}

interface SummaryActionLoadingState {
    mcp: boolean;
    assistant: boolean;
    conversation: boolean;
}

const isEnabledValue = (value: unknown) => value === true || value === "true";
const toggleCardClassName = "flex items-center justify-between rounded-md border bg-background p-4";
const nestedGroupClassName = "space-y-4 rounded-lg bg-muted/30 p-4";
const nestedSubGroupClassName = "space-y-4 rounded-lg bg-muted/40 p-4";
const actionPanelClassName =
    "flex flex-col gap-3 rounded-md border bg-background p-4 shadow-sm sm:flex-row sm:items-center sm:justify-between";
const statusPanelClassName = "space-y-2 rounded-md border bg-background p-4 shadow-sm";

const readSummaryEnabledState = (values?: Record<string, unknown>): SummaryEnabledState => ({
    mcp: isEnabledValue(values?.dynamic_mcp_loading_enabled),
    assistant: isEnabledValue(values?.assistant_summary_enabled),
    conversation: isEnabledValue(values?.conversation_summary_enabled),
});

export const ExperimentalConfigForm: React.FC<ExperimentalConfigFormProps> = ({
    form,
    onSave,
    scope = "all",
    saveFeatureConfig: saveFeatureConfigProp,
    onConfigRefresh,
}) => {
    const [isSaving, setIsSaving] = useState(false);
    const [isOnboardingOpen, setIsOnboardingOpen] = useState(false);
    const [summaryProgress, setSummaryProgress] = useState<MCPSummaryProgressPayload | null>(null);
    const [assistantSummaryProgress, setAssistantSummaryProgress] =
        useState<AssistantSummaryProgressPayload | null>(null);
    const [feishuStatus, setFeishuStatus] = useState<FeishuRuntimeStatus | null>(null);
    const [summaryTasks, setSummaryTasks] = useState<ExperimentalSummaryTaskStatus>({
        mcp_running: false,
        assistant_running: false,
        conversation_running: false,
        conversation_running_count: 0,
    });
    const [summaryActionLoading, setSummaryActionLoading] = useState<SummaryActionLoadingState>({
        mcp: false,
        assistant: false,
        conversation: false,
    });
    const defaultValues = form.formState.defaultValues as Record<string, unknown> | undefined;
    const savedSummaryEnabledRef = useRef<SummaryEnabledState>(readSummaryEnabledState(defaultValues));

    const dynamicEnabled = isEnabledValue(form.watch("dynamic_mcp_loading_enabled"));
    const assistantSummaryEnabled = isEnabledValue(form.watch("assistant_summary_enabled"));
    const conversationSummaryEnabled = isEnabledValue(form.watch("conversation_summary_enabled"));
    const butlerEnabled = isEnabledValue(form.watch("butler_experiment_enabled"));
    const feishuEnabled = isEnabledValue(form.watch("butler_feishu_enabled"));
    const summarizerModelId = form.watch("mcp_summarizer_model_id") || "";
    const assistantSummarizerModelId = form.watch("assistant_summarizer_model_id") || "";
    const conversationSummaryModel = form.watch("conversation_summary_model") || "";
    const butlerModelId = form.watch("butler_model_id") || "";
    const butlerMainWorkspacePath = String(form.watch("butler_main_workspace_path") || "");
    const butlerMainWorkspaceDescription = String(
        form.watch("butler_main_workspace_description")
        || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION
    );
    const feishuAppId = String(form.watch("butler_feishu_app_id") || "");
    const feishuAppSecret = String(form.watch("butler_feishu_app_secret") || "");
    const feishuBaseUrl = String(form.watch("butler_feishu_base_url") || "https://open.feishu.cn");
    const contextCompactionEnabled = isEnabledValue(form.watch("context_compaction_enabled"));
    const trustAllWorkspaces = isEnabledValue(form.watch("butler_trust_all_workspaces"));
    const [newTrustedPath, setNewTrustedPath] = useState("");
    const [newTrustedDesc, setNewTrustedDesc] = useState("");
    const defaultDynamicEnabled = isEnabledValue(defaultValues?.dynamic_mcp_loading_enabled);
    const defaultAssistantSummaryEnabled = isEnabledValue(defaultValues?.assistant_summary_enabled);
    const defaultConversationSummaryEnabled = isEnabledValue(defaultValues?.conversation_summary_enabled);
    const { models, loading: modelsLoading, error: modelsError } = useModels(
        dynamicEnabled || assistantSummaryEnabled || conversationSummaryEnabled || butlerEnabled
    );

    const modelOptions = useMemo(
        () =>
            models.map((model) => ({
                value: `${model.code}%%${model.llm_provider_id}`,
                label: model.name,
            })),
        [models]
    );
    const loadSummaryTaskStatus = useCallback(async () => {
        const status = await invoke<ExperimentalSummaryTaskStatus>("get_experimental_summary_task_status");
        setSummaryTasks(status);
        return status;
    }, []);

    useEffect(() => {
        savedSummaryEnabledRef.current = {
            mcp: defaultDynamicEnabled,
            assistant: defaultAssistantSummaryEnabled,
            conversation: defaultConversationSummaryEnabled,
        };
    }, [defaultAssistantSummaryEnabled, defaultConversationSummaryEnabled, defaultDynamicEnabled]);

    useEffect(() => {
        const unlistenPromise = listen<MCPSummaryProgressPayload>("mcp-summary-progress", (event) => {
            setSummaryProgress(event.payload);
            setSummaryTasks((current) => ({
                ...current,
                mcp_running: event.payload.phase !== "completed",
            }));
            if (event.payload.phase === "completed") {
                loadSummaryTaskStatus().catch(console.warn);
            }
        });
        return () => {
            unlistenPromise.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [loadSummaryTaskStatus]);

    useEffect(() => {
        const unlistenPromise = listen<AssistantSummaryProgressPayload>(
            "assistant-summary-progress",
            (event) => {
                setAssistantSummaryProgress(event.payload);
                setSummaryTasks((current) => ({
                    ...current,
                    assistant_running: event.payload.phase !== "completed",
                }));
                if (event.payload.phase === "completed") {
                    loadSummaryTaskStatus().catch(console.warn);
                }
            }
        );
        return () => {
            unlistenPromise.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [loadSummaryTaskStatus]);

    useEffect(() => {
        loadSummaryTaskStatus().catch(console.warn);
        const interval = window.setInterval(() => {
            loadSummaryTaskStatus().catch(console.warn);
        }, 5000);
        return () => {
            window.clearInterval(interval);
        };
    }, [loadSummaryTaskStatus]);

    const loadFeishuStatus = useCallback(async () => {
        const status = await invoke<FeishuRuntimeStatus>("get_butler_feishu_runtime_status");
        setFeishuStatus(status);
        return status;
    }, []);

    useEffect(() => {
        loadFeishuStatus().catch(console.warn);
        const unlistenPromise = listen<FeishuRuntimeStatus>("butler_feishu_status_changed", (event) => {
            setFeishuStatus(event.payload);
        });
        return () => {
            unlistenPromise.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, [loadFeishuStatus]);

    const triggerSummaryTask = useCallback(async (
        taskKey: SummaryTaskKey,
        options: { showToast?: boolean } = {}
    ) => {
        const { showToast = false } = options;
        setSummaryActionLoading((current) => ({ ...current, [taskKey]: true }));

        if (taskKey === "mcp") {
            setSummaryProgress({
                phase: "started",
                total: 0,
                completed: 0,
                succeeded: 0,
                failed: 0,
                server_name: undefined,
                message: "正在启动 MCP 总结...",
            });
        }

        if (taskKey === "assistant") {
            setAssistantSummaryProgress({
                phase: "started",
                total: 0,
                completed: 0,
                succeeded: 0,
                failed: 0,
                assistant_name: undefined,
                message: "正在启动助手画像生成...",
            });
        }

        const command =
            taskKey === "mcp"
                ? "trigger_mcp_summary_generation"
                : taskKey === "assistant"
                    ? "trigger_assistant_summary_generation"
                    : "trigger_conversation_summary_generation";

        try {
            const result = await invoke<SummaryTriggerResult>(command);
            await loadSummaryTaskStatus();
            if (showToast) {
                if (result.started) {
                    toast.success(result.message);
                } else {
                    toast.info(result.message);
                }
            }
            return result;
        } finally {
            setSummaryActionLoading((current) => ({ ...current, [taskKey]: false }));
        }
    }, [loadSummaryTaskStatus]);

    const autoTriggerSummaryTasks = useCallback((previousEnabledState: SummaryEnabledState, nextEnabledState: SummaryEnabledState) => {
        const tasksToTrigger: SummaryTaskKey[] = [];

        if (nextEnabledState.mcp && !previousEnabledState.mcp && !summaryTasks.mcp_running) {
            tasksToTrigger.push("mcp");
        }
        if (
            nextEnabledState.assistant
            && !previousEnabledState.assistant
            && !summaryTasks.assistant_running
        ) {
            tasksToTrigger.push("assistant");
        }
        if (
            nextEnabledState.conversation
            && !previousEnabledState.conversation
            && !summaryTasks.conversation_running
        ) {
            tasksToTrigger.push("conversation");
        }

        if (tasksToTrigger.length === 0) {
            return;
        }

        void Promise.allSettled(tasksToTrigger.map((taskKey) => triggerSummaryTask(taskKey))).then((results) => {
            const failedMessages = results.reduce<string[]>((messages, result, index) => {
                if (result.status === "rejected") {
                    messages.push(`${tasksToTrigger[index]}: ${getErrorMessage(result.reason)}`);
                }
                return messages;
            }, []);

            if (failedMessages.length > 0) {
                toast.warning(`实验性配置已保存，但自动启动摘要任务失败：${failedMessages.join("；")}`);
            }
        });
    }, [
        summaryTasks.assistant_running,
        summaryTasks.conversation_running,
        summaryTasks.mcp_running,
        triggerSummaryTask,
    ]);

    const trustedPathsRaw: string = String(form.watch("butler_trusted_workspaces") || "");
    const workspaceConfig = useMemo(
        () =>
            buildButlerWorkspaceConfig({
                mainWorkspacePath: butlerMainWorkspacePath,
                mainWorkspaceDescription: butlerMainWorkspaceDescription,
                trustedWorkspacesRaw: trustedPathsRaw,
            }),
        [butlerMainWorkspaceDescription, butlerMainWorkspacePath, trustedPathsRaw]
    );
    const trustedWorkspaces = workspaceConfig.trustedWorkspaces;
    const setTrustedWorkspaces = useCallback((ws: TrustedWorkspace[]) => {
        const nextConfig = serializeButlerWorkspaceConfig({
            mainWorkspacePath: butlerMainWorkspacePath,
            mainWorkspaceDescription: butlerMainWorkspaceDescription,
            trustedWorkspaces: ws,
        });
        form.setValue("butler_main_workspace_path", nextConfig.mainWorkspacePath, {
            shouldDirty: true,
        });
        form.setValue(
            "butler_main_workspace_description",
            nextConfig.mainWorkspaceDescription || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
            { shouldDirty: true }
        );
        form.setValue("butler_trusted_workspaces", nextConfig.trustedWorkspacesRaw, {
            shouldDirty: true,
        });
    }, [butlerMainWorkspaceDescription, butlerMainWorkspacePath, form]);
    const setMainWorkspace = useCallback((path: string, description: string) => {
        const nextConfig = serializeButlerWorkspaceConfig({
            mainWorkspacePath: path,
            mainWorkspaceDescription: description,
            trustedWorkspaces,
        });
        form.setValue("butler_main_workspace_path", nextConfig.mainWorkspacePath, {
            shouldDirty: true,
        });
        form.setValue(
            "butler_main_workspace_description",
            nextConfig.mainWorkspaceDescription || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
            { shouldDirty: true }
        );
        form.setValue("butler_trusted_workspaces", nextConfig.trustedWorkspacesRaw, {
            shouldDirty: true,
        });
    }, [form, trustedWorkspaces]);
    const handleAddTrustedPath = useCallback(() => {
        const trimmed = newTrustedPath.trim();
        if (!trimmed) return;
        if (trimmed === butlerMainWorkspacePath.trim()) {
            toast.error("该路径已被设置为主工作区");
            return;
        }
        if (trustedWorkspaces.some(w => w.path === trimmed)) {
            toast.error("该路径已存在");
            return;
        }
        setTrustedWorkspaces([...trustedWorkspaces, { path: trimmed, description: newTrustedDesc.trim() }]);
        setNewTrustedPath("");
        setNewTrustedDesc("");
    }, [
        butlerMainWorkspacePath,
        newTrustedDesc,
        newTrustedPath,
        setTrustedWorkspaces,
        trustedWorkspaces,
    ]);
    const handleRemoveTrustedPath = useCallback((path: string) => {
        setTrustedWorkspaces(trustedWorkspaces.filter(w => w.path !== path));
    }, [trustedWorkspaces, setTrustedWorkspaces]);
    const handleUpdateDescription = useCallback((path: string, desc: string) => {
        setTrustedWorkspaces(trustedWorkspaces.map(w => w.path === path ? { ...w, description: desc } : w));
    }, [trustedWorkspaces, setTrustedWorkspaces]);
    const showSummarySection = scope === "all";

    const handleSave = useCallback(async () => {
        if (showSummarySection && dynamicEnabled && !summarizerModelId) {
            toast.error("请先选择 MCP 总结 AI 模型后再保存");
            return;
        }
        if (showSummarySection && assistantSummaryEnabled && !assistantSummarizerModelId) {
            toast.error("请先选择助手总结 AI 模型后再保存");
            return;
        }
        if (showSummarySection && conversationSummaryEnabled && !conversationSummaryModel) {
            toast.error("请先选择对话总结模型后再保存");
            return;
        }
        if (butlerEnabled && !butlerModelId) {
            toast.error("请先为总管家模式选择模型");
            return;
        }
        if (butlerEnabled && !butlerMainWorkspacePath.trim()) {
            toast.error("请先配置总管家的主工作区");
            return;
        }
        if (butlerEnabled && feishuEnabled && !feishuAppId.trim()) {
            toast.error("请先填写飞书 App ID");
            return;
        }
        if (
            butlerEnabled
            && feishuEnabled
            && !feishuAppSecret.trim()
            && !feishuStatus?.secret_configured
        ) {
            toast.error("请先填写飞书 App Secret");
            return;
        }

        const previousEnabledState = savedSummaryEnabledRef.current;
        const nextEnabledState: SummaryEnabledState = {
            mcp: dynamicEnabled,
            assistant: assistantSummaryEnabled,
            conversation: conversationSummaryEnabled,
        };

        setIsSaving(true);
        try {
            await onSave();
            if (butlerEnabled && feishuEnabled && feishuAppSecret.trim()) {
                await invoke("save_butler_feishu_secret", {
                    appSecret: feishuAppSecret.trim(),
                    app_secret: feishuAppSecret.trim(),
                });
                form.setValue("butler_feishu_app_secret", "");
            }
            const latestFeishuStatus = await invoke<FeishuRuntimeStatus>(
                "refresh_butler_feishu_runtime_command"
            );
            setFeishuStatus(latestFeishuStatus);
            savedSummaryEnabledRef.current = nextEnabledState;
            toast.success("实验性配置保存成功");
            autoTriggerSummaryTasks(previousEnabledState, nextEnabledState);
        } catch (error) {
            toast.error("保存实验性配置失败: " + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    }, [
        assistantSummarizerModelId,
        assistantSummaryEnabled,
        autoTriggerSummaryTasks,
        butlerEnabled,
        butlerModelId,
        conversationSummaryEnabled,
        conversationSummaryModel,
        dynamicEnabled,
        feishuAppId,
        feishuAppSecret,
        feishuEnabled,
        feishuStatus?.secret_configured,
        form,
        onSave,
        showSummarySection,
        summarizerModelId,
    ]);

    const handleRunMcpSummary = useCallback(async () => {
        if (!summarizerModelId) {
            toast.error("请先选择 MCP 总结 AI 模型");
            return;
        }

        try {
            await triggerSummaryTask("mcp", { showToast: true });
        } catch (error) {
            toast.error("启动 MCP 总结失败: " + getErrorMessage(error));
        }
    }, [summarizerModelId, triggerSummaryTask]);

    const handleRunAssistantSummary = useCallback(async () => {
        if (!assistantSummarizerModelId) {
            toast.error("请先选择助手总结 AI 模型");
            return;
        }

        try {
            await triggerSummaryTask("assistant", { showToast: true });
        } catch (error) {
            toast.error("启动助手画像生成失败: " + getErrorMessage(error));
        }
    }, [assistantSummarizerModelId, triggerSummaryTask]);

    const handleRunConversationSummary = useCallback(async () => {
        if (!conversationSummaryModel) {
            toast.error("请先选择对话总结模型");
            return;
        }

        try {
            await triggerSummaryTask("conversation", { showToast: true });
        } catch (error) {
            toast.error("启动对话总结失败: " + getErrorMessage(error));
        }
    }, [conversationSummaryModel, triggerSummaryTask]);

    const progressValue =
        summaryProgress && summaryProgress.total > 0
            ? Math.round((summaryProgress.completed / summaryProgress.total) * 100)
            : 0;
    const assistantSummaryProgressValue =
        assistantSummaryProgress && assistantSummaryProgress.total > 0
            ? Math.round((assistantSummaryProgress.completed / assistantSummaryProgress.total) * 100)
            : 0;
    const mcpSummaryRunning = summaryTasks.mcp_running || summaryActionLoading.mcp;
    const assistantSummaryRunning = summaryTasks.assistant_running || summaryActionLoading.assistant;
    const conversationSummaryRunning =
        summaryTasks.conversation_running || summaryActionLoading.conversation;
    const handleRefreshFeishu = useCallback(async () => {
        setIsSaving(true);
        try {
            const status = await invoke<FeishuRuntimeStatus>("refresh_butler_feishu_runtime_command");
            setFeishuStatus(status);
            toast.success("飞书机器人已重载");
        } catch (error) {
            toast.error("重连飞书机器人失败: " + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    }, []);

    const handleClearFeishuSecret = useCallback(async () => {
        setIsSaving(true);
        try {
            await invoke("clear_butler_feishu_secret");
            form.setValue("butler_feishu_app_secret", "");
            const status = await invoke<FeishuRuntimeStatus>("refresh_butler_feishu_runtime_command");
            setFeishuStatus(status);
            toast.success("飞书 App Secret 已清除");
        } catch (error) {
            toast.error("清除飞书密钥失败: " + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    }, [form]);

    const saveDisabled =
        isSaving
        || (showSummarySection && dynamicEnabled && !summarizerModelId)
        || (showSummarySection && assistantSummaryEnabled && !assistantSummarizerModelId)
        || (showSummarySection && conversationSummaryEnabled && !conversationSummaryModel)
        || (butlerEnabled && !butlerModelId)
        || (butlerEnabled && !butlerMainWorkspacePath.trim())
        || (butlerEnabled && feishuEnabled && !feishuAppId.trim())
        || (butlerEnabled && feishuEnabled && !feishuAppSecret.trim() && !feishuStatus?.secret_configured);

    return (
        <Form {...form}>
            <Card
                className={
                    showSummarySection
                        ? "bottom-space border-l-4 border-l-primary shadow-none"
                        : "border-0 shadow-none"
                }
            >
                {showSummarySection ? (
                    <CardHeader>
                        <CardTitle className="text-lg font-semibold">实验性功能</CardTitle>
                        <p className="text-sm text-muted-foreground">
                            新能力可能存在兼容风险，请按需启用。
                        </p>
                    </CardHeader>
                ) : null}
                <CardContent className="space-y-6">
                    {showSummarySection ? (
                        <div className="space-y-4">
                            <div>
                                <h3 className="text-sm font-medium">摘要与动态加载</h3>
                                <p className="mt-1 text-sm text-muted-foreground">
                                    将相关实验能力放在一起，先开关、后选模型、再手动触发；首次从关闭切到开启后，保存会自动在后台补跑一次。
                                </p>
                            </div>

                            <div className="space-y-4">
                                <Controller
                                    control={form.control}
                                    name="dynamic_mcp_loading_enabled"
                                    render={({ field }) => (
                                        <FormItem className={toggleCardClassName}>
                                            <div>
                                                <FormLabel className="text-base">MCP 动态加载（实验）</FormLabel>
                                                <p className="mt-1 text-sm text-muted-foreground">
                                                    开启后采用 MCP 目录摘要 + 按需加载模式。
                                                </p>
                                            </div>
                                            <FormControl>
                                                <Switch
                                                    checked={isEnabledValue(field.value)}
                                                    onCheckedChange={field.onChange}
                                                />
                                            </FormControl>
                                        </FormItem>
                                    )}
                                />

                            {dynamicEnabled && (
                                <div className={nestedGroupClassName}>
                                    <Controller
                                        control={form.control}
                                        name="mcp_summarizer_model_id"
                                        render={({ field }) => (
                                            <FormItem>
                                                <FormLabel>MCP 总结 AI</FormLabel>
                                                <FormControl>
                                                    <Select
                                                        value={field.value || ""}
                                                        onValueChange={field.onChange}
                                                        disabled={modelsLoading}
                                                    >
                                                        <SelectTrigger>
                                                            <SelectValue
                                                                placeholder={
                                                                    modelsLoading
                                                                        ? "加载中..."
                                                                        : modelsError
                                                                            ? "加载失败"
                                                                            : "选择 MCP 总结模型"
                                                                }
                                                            />
                                                        </SelectTrigger>
                                                        <SelectContent>
                                                            {modelOptions.map((option) => (
                                                                <SelectItem key={option.value} value={option.value}>
                                                                    {option.label}
                                                                </SelectItem>
                                                            ))}
                                                        </SelectContent>
                                                    </Select>
                                                </FormControl>
                                                <p className="mt-1 text-sm text-muted-foreground">
                                                    仅在首次启用后保存时自动触发一次；后续如需重跑，可使用下面的按钮单独触发。
                                                </p>
                                                <FormMessage />
                                            </FormItem>
                                        )}
                                    />

                                    <div className={actionPanelClassName}>
                                        <div>
                                            <p className="text-sm font-medium">MCP 总结任务</p>
                                            <p className="mt-1 text-xs text-muted-foreground">
                                                不影响保存流程，后台任务会通过进度条持续更新。
                                            </p>
                                        </div>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            onClick={handleRunMcpSummary}
                                            disabled={
                                                isSaving
                                                || summaryActionLoading.mcp
                                                || summaryTasks.mcp_running
                                                || !summarizerModelId
                                            }
                                        >
                                            {summaryActionLoading.mcp
                                                ? "启动中..."
                                                : summaryTasks.mcp_running
                                                    ? "MCP 总结进行中"
                                                    : "立即生成 MCP 总结"}
                                        </Button>
                                    </div>

                                    {(summaryProgress || mcpSummaryRunning) && (
                                        <div className={statusPanelClassName}>
                                            <div className="flex items-center justify-between text-sm">
                                                <span className="font-medium">MCP 总结进度</span>
                                                <span>
                                                    {summaryProgress && summaryProgress.total > 0
                                                        ? `${summaryProgress.completed}/${summaryProgress.total}`
                                                        : mcpSummaryRunning
                                                            ? "启动中"
                                                            : "空闲"}
                                                </span>
                                            </div>
                                            <Progress value={summaryProgress ? progressValue : 0} />
                                            <p className="text-xs text-muted-foreground">
                                                {summaryProgress
                                                    ? summaryProgress.phase === "processing"
                                                        ? `当前: ${summaryProgress.server_name || "处理中"}`
                                                        : summaryProgress.phase === "completed"
                                                            ? `完成: 成功 ${summaryProgress.succeeded}，失败 ${summaryProgress.failed}`
                                                            : summaryProgress.message || "准备中..."
                                                    : "后台任务正在启动，请稍候..."}
                                            </p>
                                        </div>
                                    )}
                                </div>
                            )}

                            <Controller
                                control={form.control}
                                name="assistant_summary_enabled"
                                render={({ field }) => (
                                    <FormItem className={toggleCardClassName}>
                                        <div>
                                            <FormLabel className="text-base">助手总结（实验）</FormLabel>
                                            <p className="mt-1 text-sm text-muted-foreground">
                                                根据助手名称、提示词、MCP 和 Skills 生成执行画像，并注入到总管家 prompt。
                                            </p>
                                        </div>
                                        <FormControl>
                                            <Switch
                                                checked={isEnabledValue(field.value)}
                                                onCheckedChange={field.onChange}
                                            />
                                        </FormControl>
                                    </FormItem>
                                )}
                            />

                            {assistantSummaryEnabled && (
                                <div className={nestedGroupClassName}>
                                    <Controller
                                        control={form.control}
                                        name="assistant_summarizer_model_id"
                                        render={({ field }) => (
                                            <FormItem>
                                                <FormLabel>助手总结 AI</FormLabel>
                                                <FormControl>
                                                    <Select
                                                        value={field.value || ""}
                                                        onValueChange={field.onChange}
                                                        disabled={modelsLoading}
                                                    >
                                                        <SelectTrigger>
                                                            <SelectValue
                                                                placeholder={
                                                                    modelsLoading
                                                                        ? "加载中..."
                                                                        : modelsError
                                                                            ? "加载失败"
                                                                            : "选择助手总结模型"
                                                                }
                                                            />
                                                        </SelectTrigger>
                                                        <SelectContent>
                                                            {modelOptions.map((option) => (
                                                                <SelectItem key={option.value} value={option.value}>
                                                                    {option.label}
                                                                </SelectItem>
                                                            ))}
                                                        </SelectContent>
                                                    </Select>
                                                </FormControl>
                                                <p className="mt-1 text-sm text-muted-foreground">
                                                    首次启用后保存会自动补跑；后续可以在不重新保存配置的情况下单独触发。
                                                </p>
                                                <FormMessage />
                                            </FormItem>
                                        )}
                                    />

                                    <div className={actionPanelClassName}>
                                        <div>
                                            <p className="text-sm font-medium">助手画像任务</p>
                                            <p className="mt-1 text-xs text-muted-foreground">
                                                会在后台按顺序刷新普通助手画像，并实时同步到进度条。
                                            </p>
                                        </div>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            onClick={handleRunAssistantSummary}
                                            disabled={
                                                isSaving
                                                || summaryActionLoading.assistant
                                                || summaryTasks.assistant_running
                                                || !assistantSummarizerModelId
                                            }
                                        >
                                            {summaryActionLoading.assistant
                                                ? "启动中..."
                                                : summaryTasks.assistant_running
                                                    ? "助手画像生成中"
                                                    : "立即生成助手画像"}
                                        </Button>
                                    </div>

                                    {(assistantSummaryProgress || assistantSummaryRunning) && (
                                        <div className={statusPanelClassName}>
                                            <div className="flex items-center justify-between text-sm">
                                                <span className="font-medium">助手画像进度</span>
                                                <span>
                                                    {assistantSummaryProgress && assistantSummaryProgress.total > 0
                                                        ? `${assistantSummaryProgress.completed}/${assistantSummaryProgress.total}`
                                                        : assistantSummaryRunning
                                                            ? "启动中"
                                                            : "空闲"}
                                                </span>
                                            </div>
                                            <Progress value={assistantSummaryProgress ? assistantSummaryProgressValue : 0} />
                                            <p className="text-xs text-muted-foreground">
                                                {assistantSummaryProgress
                                                    ? assistantSummaryProgress.phase === "processing"
                                                        ? `当前: ${assistantSummaryProgress.assistant_name || "处理中"}`
                                                        : assistantSummaryProgress.phase === "completed"
                                                            ? `完成: 成功 ${assistantSummaryProgress.succeeded}，失败 ${assistantSummaryProgress.failed}`
                                                            : assistantSummaryProgress.message || "准备中..."
                                                    : "后台任务正在启动，请稍候..."}
                                            </p>
                                        </div>
                                    )}
                                </div>
                            )}

                            <Controller
                                control={form.control}
                                name="conversation_summary_enabled"
                                render={({ field }) => (
                                    <FormItem className={toggleCardClassName}>
                                        <div>
                                            <FormLabel className="text-base">对话总结（实验）</FormLabel>
                                            <p className="mt-1 text-sm text-muted-foreground">
                                                对话空闲一段时间后自动生成摘要，用于搜索、回顾和后续上下文重建。
                                            </p>
                                        </div>
                                        <FormControl>
                                            <Switch
                                                checked={isEnabledValue(field.value)}
                                                onCheckedChange={field.onChange}
                                            />
                                        </FormControl>
                                    </FormItem>
                                )}
                            />

                            {conversationSummaryEnabled && (
                                <div className={nestedGroupClassName}>
                                    <Controller
                                        control={form.control}
                                        name="conversation_summary_model"
                                        render={({ field }) => (
                                            <FormItem>
                                                <FormLabel>对话总结模型</FormLabel>
                                                <FormControl>
                                                    <Select
                                                        value={field.value || ""}
                                                        onValueChange={field.onChange}
                                                        disabled={modelsLoading}
                                                    >
                                                        <SelectTrigger>
                                                            <SelectValue
                                                                placeholder={
                                                                    modelsLoading
                                                                        ? "加载中..."
                                                                        : modelsError
                                                                            ? "加载失败"
                                                                            : "选择对话总结模型"
                                                                }
                                                            />
                                                        </SelectTrigger>
                                                        <SelectContent>
                                                            {modelOptions.map((option) => (
                                                                <SelectItem key={option.value} value={option.value}>
                                                                    {option.label}
                                                                </SelectItem>
                                                            ))}
                                                        </SelectContent>
                                                    </Select>
                                                </FormControl>
                                                <p className="mt-1 text-sm text-muted-foreground">
                                                    该配置会优先覆盖旧的“辅助AI → 对话总结”设置。
                                                </p>
                                                <FormMessage />
                                            </FormItem>
                                        )}
                                    />

                                    <div className={actionPanelClassName}>
                                        <div>
                                            <p className="text-sm font-medium">对话总结任务</p>
                                            <p className="mt-1 text-xs text-muted-foreground">
                                                点击后会立即扫描当前需要处理的对话，并在后台启动对应总结任务。
                                            </p>
                                        </div>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            onClick={handleRunConversationSummary}
                                            disabled={
                                                isSaving
                                                || summaryActionLoading.conversation
                                                || summaryTasks.conversation_running
                                                || !conversationSummaryModel
                                            }
                                        >
                                            {summaryActionLoading.conversation
                                                ? "启动中..."
                                                : summaryTasks.conversation_running
                                                    ? "对话总结进行中"
                                                    : "立即触发对话总结"}
                                        </Button>
                                    </div>

                                    <div className={statusPanelClassName}>
                                        <div className="flex items-center justify-between text-sm">
                                            <span className="font-medium">对话总结后台状态</span>
                                            <span>
                                                {conversationSummaryRunning
                                                    ? summaryTasks.conversation_running_count > 0
                                                        ? `${summaryTasks.conversation_running_count} 个任务运行中`
                                                        : "启动中"
                                                    : "空闲"}
                                            </span>
                                        </div>
                                        <p className="text-xs text-muted-foreground">
                                            {conversationSummaryRunning
                                                ? summaryTasks.conversation_running_count > 0
                                                    ? `已有 ${summaryTasks.conversation_running_count} 个对话总结任务正在后台执行。`
                                                    : "对话总结任务正在启动，请稍候..."
                                                : "保存配置不会等待该后台任务完成。"}
                                        </p>
                                    </div>
                                </div>
                            )}
                            </div>
                        </div>
                    ) : null}

                    <div className="space-y-4">
                        <div>
                            <h3 className="text-sm font-medium">总管家与飞书接入</h3>
                            <p className="mt-1 text-sm text-muted-foreground">
                                先开启总管家模式，再配置模型、主页和飞书连接等依赖项。
                            </p>
                        </div>

                        <Controller
                            control={form.control}
                            name="butler_experiment_enabled"
                            render={({ field }) => (
                                <FormItem className={toggleCardClassName}>
                                    <div>
                                        <FormLabel className="text-base">总管家模式（实验）</FormLabel>
                                        <p className="mt-1 text-sm text-muted-foreground">
                                            开启后新增独立的总管家工作台、隐藏任务会话和任务结果聚合视图。
                                        </p>
                                    </div>
                                    <FormControl>
                                        <Switch
                                            checked={isEnabledValue(field.value)}
                                            onCheckedChange={field.onChange}
                                        />
                                    </FormControl>
                                </FormItem>
                            )}
                        />

                        {butlerEnabled && (
                            <div className={nestedGroupClassName}>
                                {saveFeatureConfigProp && (
                                    <div className="flex items-center gap-3 rounded-lg border border-primary/30 bg-primary/5 p-3">
                                        <div className="flex-1 space-y-0.5">
                                            <p className="text-sm font-medium">引导配置</p>
                                            <p className="text-xs text-muted-foreground">
                                                通过分步向导快速完成总管家的模型、环境、工作区和飞书配置。
                                            </p>
                                        </div>
                                        <Button
                                            type="button"
                                            variant="outline"
                                            size="sm"
                                            onClick={() => setIsOnboardingOpen(true)}
                                        >
                                            <Wand2 className="h-4 w-4 mr-1.5" />
                                            开始引导
                                        </Button>
                                    </div>
                                )}
                                <Controller
                                    control={form.control}
                                    name="butler_model_id"
                                    render={({ field }) => (
                                        <FormItem>
                                            <FormLabel>总管家模型</FormLabel>
                                            <FormControl>
                                                <Select
                                                    value={field.value || ""}
                                                    onValueChange={field.onChange}
                                                    disabled={modelsLoading}
                                                >
                                                    <SelectTrigger>
                                                        <SelectValue
                                                            placeholder={
                                                                modelsLoading
                                                                    ? "加载中..."
                                                                    : modelsError
                                                                        ? "加载失败"
                                                                        : "选择总管家模型"
                                                            }
                                                        />
                                                    </SelectTrigger>
                                                    <SelectContent>
                                                        {modelOptions.map((option) => (
                                                            <SelectItem
                                                                key={option.value}
                                                                value={option.value}
                                                            >
                                                                {option.label}
                                                            </SelectItem>
                                                        ))}
                                                    </SelectContent>
                                                </Select>
                                            </FormControl>
                                            <p className="text-sm text-muted-foreground mt-1">
                                                总管家主会话会使用固定内置提示词，并绑定到这里选择的模型。
                                            </p>
                                            <FormMessage />
                                        </FormItem>
                                    )}
                                />

                                <Controller
                                    control={form.control}
                                    name="butler_display_name"
                                    render={({ field }) => (
                                        <FormItem>
                                            <FormLabel>总管家显示名称</FormLabel>
                                            <FormControl>
                                                <Input
                                                    value={field.value || ""}
                                                    onChange={field.onChange}
                                                    placeholder="总管家"
                                                />
                                            </FormControl>
                                            <p className="mt-1 text-sm text-muted-foreground">
                                                用于总管家工作台头部展示；留空时回退为“总管家”。
                                            </p>
                                            <FormMessage />
                                        </FormItem>
                                    )}
                                />

                                <Controller
                                    control={form.control}
                                    name="default_home_window"
                                    render={({ field }) => (
                                        <FormItem>
                                            <FormLabel>默认主页窗口</FormLabel>
                                            <FormControl>
                                                <Select
                                                    value={field.value || "ask"}
                                                    onValueChange={field.onChange}
                                                >
                                                    <SelectTrigger>
                                                        <SelectValue placeholder="选择默认主页窗口" />
                                                    </SelectTrigger>
                                                    <SelectContent>
                                                        <SelectItem value="ask">Ask 悬浮窗</SelectItem>
                                                        <SelectItem value="chat_ui">Chat 主窗口</SelectItem>
                                                        <SelectItem value="butler_experiment">
                                                            总管家实验窗口
                                                        </SelectItem>
                                                    </SelectContent>
                                                </Select>
                                            </FormControl>
                                            <p className="text-sm text-muted-foreground mt-1">
                                                影响应用启动、托盘点击和唤醒时默认打开的主窗口。
                                            </p>
                                        </FormItem>
                                    )}
                                />

                                {/* ── 上下文压缩配置 ── */}
                                <div className="space-y-3 pt-3 border-t border-border/50">
                                    <p className="text-sm font-medium text-muted-foreground">上下文压缩</p>

                                    <Controller
                                        control={form.control}
                                        name="context_compaction_enabled"
                                        render={({ field }) => (
                                            <FormItem className={toggleCardClassName}>
                                                <div>
                                                    <FormLabel className="text-base">自动上下文压缩</FormLabel>
                                                    <p className="text-sm text-muted-foreground mt-1">
                                                        当对话上下文接近模型窗口上限时，自动通过 LLM 摘要压缩历史消息。
                                                    </p>
                                                </div>
                                                <FormControl>
                                                    <Switch
                                                        checked={isEnabledValue(field.value)}
                                                        onCheckedChange={field.onChange}
                                                    />
                                                </FormControl>
                                            </FormItem>
                                        )}
                                    />

                                    {contextCompactionEnabled && (
                                        <div className={nestedGroupClassName}>
                                            <Controller
                                                control={form.control}
                                                name="context_max_input_tokens"
                                                render={({ field }) => (
                                                    <FormItem>
                                                        <FormLabel>模型上下文窗口</FormLabel>
                                                        <FormControl>
                                                            <Input
                                                                type="number"
                                                                value={field.value || "128000"}
                                                                onChange={field.onChange}
                                                                placeholder="128000"
                                                            />
                                                        </FormControl>
                                                        <FormDescription>
                                                            模型的上下文窗口总大小（含输入与输出），系统会自动预留输出空间。
                                                        </FormDescription>
                                                    </FormItem>
                                                )}
                                            />
                                            <Controller
                                                control={form.control}
                                                name="context_compaction_threshold"
                                                render={({ field }) => (
                                                    <FormItem>
                                                        <FormLabel>压缩触发比例</FormLabel>
                                                        <FormControl>
                                                            <Input
                                                                type="number"
                                                                step="0.05"
                                                                min="0.5"
                                                                max="0.99"
                                                                value={field.value || "0.80"}
                                                                onChange={field.onChange}
                                                                placeholder="0.80"
                                                            />
                                                        </FormControl>
                                                        <FormDescription>
                                                            当 Token 使用量达到此比例时触发自动压缩（0.5 ~ 0.99）。
                                                        </FormDescription>
                                                    </FormItem>
                                                )}
                                            />
                                            <Controller
                                                control={form.control}
                                                name="context_tail_ratio"
                                                render={({ field }) => (
                                                    <FormItem>
                                                        <FormLabel>尾部保留比例</FormLabel>
                                                        <FormControl>
                                                            <Input
                                                                type="number"
                                                                step="0.05"
                                                                min="0.05"
                                                                max="0.80"
                                                                value={field.value || "0.30"}
                                                                onChange={field.onChange}
                                                                placeholder="0.30"
                                                            />
                                                        </FormControl>
                                                        <FormDescription>
                                                            为最近消息保留的上下文预算比例。系统按 Token 从最新消息往前自动计算保留条数，永远不会超出上下文容量（0.05 ~ 0.80）。
                                                        </FormDescription>
                                                    </FormItem>
                                                )}
                                            />
                                        </div>
                                    )}
                                </div>

                                {/* ── 可信工作区配置 ── */}
                                <div className="space-y-3 pt-3 border-t border-border/50">
                                    <p className="text-sm font-medium text-muted-foreground">可信工作区</p>

                                    <Controller
                                        control={form.control}
                                        name="butler_trust_all_workspaces"
                                        render={({ field }) => (
                                            <FormItem className={toggleCardClassName}>
                                                <div>
                                                    <FormLabel className="text-base flex items-center gap-2">
                                                        信任任何工作区
                                                        <span className="inline-flex items-center gap-1 rounded-md bg-destructive/10 px-2 py-0.5 text-xs font-medium text-destructive">
                                                            <AlertTriangle className="h-3 w-3" />
                                                            危险
                                                        </span>
                                                    </FormLabel>
                                                    <p className="text-sm text-muted-foreground mt-1">
                                                        开启后总管家对所有路径的文件操作将自动放行，不再弹出任何权限确认弹窗。仅建议在完全受信环境下使用。
                                                    </p>
                                                </div>
                                                <FormControl>
                                                    <Switch
                                                        checked={isEnabledValue(field.value)}
                                                        onCheckedChange={field.onChange}
                                                    />
                                                </FormControl>
                                            </FormItem>
                                        )}
                                    />

                                    <div className={nestedGroupClassName}>
                                        <p className="text-sm text-muted-foreground">
                                            主工作区为必填，额外工作区按需补充。在这些路径下的文件操作将自动放行，描述会注入到总管家的提示词中帮助 AI 理解工作区用途。
                                        </p>
                                        <div className="space-y-3 rounded-lg border border-primary/20 bg-background p-4">
                                            <div>
                                                <p className="text-sm font-medium">主工作区</p>
                                                <p className="mt-1 text-xs text-muted-foreground">
                                                    总管家会优先在这里组织任务、文件与产物。
                                                </p>
                                            </div>
                                            <FolderPicker
                                                value={butlerMainWorkspacePath}
                                                onChange={(value) =>
                                                    setMainWorkspace(
                                                        value,
                                                        butlerMainWorkspaceDescription
                                                    )
                                                }
                                                placeholder="选择或输入主工作区目录路径"
                                            />
                                            <Input
                                                value={butlerMainWorkspaceDescription}
                                                onChange={(event) =>
                                                    setMainWorkspace(
                                                        butlerMainWorkspacePath,
                                                        event.target.value
                                                    )
                                                }
                                                placeholder={BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION}
                                            />
                                            {!butlerMainWorkspacePath.trim() ? (
                                                <p className="text-xs text-destructive">
                                                    请先配置主工作区
                                                </p>
                                            ) : null}
                                        </div>

                                        {!trustAllWorkspaces && (
                                            <>
                                                <div className="space-y-2">
                                                    <p className="text-sm font-medium">额外可信工作区</p>
                                                    <p className="text-xs text-muted-foreground">
                                                        用于补充主工作区之外也允许自动放行的目录。
                                                    </p>
                                                </div>

                                            <div className="space-y-2">
                                                <div className="flex items-center gap-2">
                                                    <FolderPicker
                                                        value={newTrustedPath}
                                                        onChange={setNewTrustedPath}
                                                        placeholder="选择或输入额外可信目录路径"
                                                    />
                                                    <Button
                                                        type="button"
                                                        size="sm"
                                                        onClick={handleAddTrustedPath}
                                                        disabled={!newTrustedPath.trim()}
                                                    >
                                                        <Plus className="h-4 w-4 mr-1" />
                                                        添加
                                                    </Button>
                                                </div>
                                                <Input
                                                    value={newTrustedDesc}
                                                    onChange={(e) => setNewTrustedDesc(e.target.value)}
                                                    placeholder="工作区描述（可选），例如：前端项目、Rust 后端代码仓库"
                                                    className="text-sm"
                                                />
                                            </div>
                                            {/* 已有工作区列表 */}
                                            <div className="space-y-2 max-h-64 overflow-y-auto">
                                                {trustedWorkspaces.length === 0 ? (
                                                    <div className="text-sm text-muted-foreground text-center py-3">
                                                        暂未配置额外可信工作区
                                                    </div>
                                                ) : (
                                                    trustedWorkspaces.map((ws) => (
                                                        <div
                                                            key={ws.path}
                                                            className="p-2 bg-background rounded-md border space-y-1"
                                                        >
                                                            <div className="flex items-center justify-between">
                                                                <span className="text-sm font-mono break-all flex-1 mr-2">
                                                                    {ws.path}
                                                                </span>
                                                                <Button
                                                                    type="button"
                                                                    variant="ghost"
                                                                    size="sm"
                                                                    onClick={() => handleRemoveTrustedPath(ws.path)}
                                                                    className="text-destructive hover:text-destructive shrink-0"
                                                                >
                                                                    <Trash2 className="h-4 w-4" />
                                                                </Button>
                                                            </div>
                                                            <Input
                                                                value={ws.description}
                                                                onChange={(e) => handleUpdateDescription(ws.path, e.target.value)}
                                                                placeholder="添加描述…"
                                                                className="text-xs h-7"
                                                            />
                                                        </div>
                                                    ))
                                                )}
                                            </div>
                                            </>
                                        )}
                                    </div>
                                </div>

                                <Controller
                                    control={form.control}
                                    name="butler_feishu_enabled"
                                    render={({ field }) => (
                                        <FormItem className={toggleCardClassName}>
                                            <div>
                                                <FormLabel className="text-base">飞书机器人接入（实验）</FormLabel>
                                                <p className="text-sm text-muted-foreground mt-1">
                                                    开启后总管家会通过飞书长连接接收文本消息，并把最终答复回发到飞书。
                                                </p>
                                            </div>
                                            <FormControl>
                                                <Switch
                                                    checked={field.value === true || field.value === "true"}
                                                    onCheckedChange={field.onChange}
                                                />
                                            </FormControl>
                                        </FormItem>
                                    )}
                                />

                                {feishuEnabled && (
                                    <div className={nestedSubGroupClassName}>
                                        <div className={statusPanelClassName}>
                                            <div className="flex items-center justify-between gap-3">
                                                <div>
                                                    <p className="text-sm font-medium">飞书机器人状态</p>
                                                    <p className="text-xs text-muted-foreground mt-1">
                                                        {feishuStatus?.status_text || "正在读取状态..."}
                                                    </p>
                                                </div>
                                                <div className="flex gap-2">
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        onClick={handleRefreshFeishu}
                                                        disabled={isSaving}
                                                    >
                                                        重连机器人
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        onClick={handleClearFeishuSecret}
                                                        disabled={isSaving || !feishuStatus?.secret_configured}
                                                    >
                                                        清除密钥
                                                    </Button>
                                                </div>
                                            </div>
                                            <div className="grid gap-2 text-xs text-muted-foreground md:grid-cols-2">
                                                <div>App ID：{feishuStatus?.app_id || feishuAppId || "未配置"}</div>
                                                <div>Secret：{feishuStatus?.secret_configured ? "已保存" : "未保存"}</div>
                                                <div>连接：{feishuStatus?.connected ? "已连接" : "未连接"}</div>
                                                <div>最近事件：{feishuStatus?.last_event_at || "暂无"}</div>
                                                <div className="md:col-span-2">
                                                    当前阶段：{feishuStatus?.status_detail || "暂无详细状态"}
                                                </div>
                                                <div className="md:col-span-2">
                                                    状态更新时间：{feishuStatus?.last_status_at || "暂无"}
                                                </div>
                                                {feishuStatus?.last_error && (
                                                    <div className="md:col-span-2 text-destructive">
                                                        最近错误：{feishuStatus.last_error}
                                                    </div>
                                                )}
                                            </div>
                                        </div>

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_app_id"
                                            render={({ field }) => (
                                                <FormItem>
                                                    <FormLabel>飞书 App ID</FormLabel>
                                                    <FormControl>
                                                        <Input {...field} value={field.value || ""} placeholder="cli_xxx" />
                                                    </FormControl>
                                                    <FormDescription>
                                                        使用飞书自建应用机器人的 App ID。
                                                    </FormDescription>
                                                    <FormMessage />
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_app_secret"
                                            render={({ field }) => (
                                                <FormItem>
                                                    <FormLabel>飞书 App Secret</FormLabel>
                                                    <FormControl>
                                                        <Input
                                                            {...field}
                                                            type="password"
                                                            value={field.value || ""}
                                                            placeholder={
                                                                feishuStatus?.secret_configured
                                                                    ? "已保存，留空则保持不变"
                                                                    : "输入新的 App Secret"
                                                            }
                                                        />
                                                    </FormControl>
                                                    <FormDescription>
                                                        仅在点击“保存配置”时单独加密保存，不会进入普通 experimental 配置。
                                                    </FormDescription>
                                                    <FormMessage />
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_base_url"
                                            render={({ field }) => (
                                                <FormItem>
                                                    <FormLabel>飞书开放平台域名</FormLabel>
                                                    <FormControl>
                                                        <Select value={field.value || feishuBaseUrl} onValueChange={field.onChange}>
                                                            <SelectTrigger>
                                                                <SelectValue placeholder="选择飞书开放平台域名" />
                                                            </SelectTrigger>
                                                            <SelectContent>
                                                                <SelectItem value="https://open.feishu.cn">
                                                                    飞书（中国大陆）
                                                                </SelectItem>
                                                                <SelectItem value="https://open.larksuite.com">
                                                                    Lark（国际版）
                                                                </SelectItem>
                                                            </SelectContent>
                                                        </Select>
                                                    </FormControl>
                                                    <FormMessage />
                                                </FormItem>
                                            )}
                                        />

                                        <div className="grid gap-4 md:grid-cols-2">
                                            <Controller
                                                control={form.control}
                                                name="butler_feishu_receive_p2p"
                                                render={({ field }) => (
                                                    <FormItem className={toggleCardClassName}>
                                                        <div>
                                                            <FormLabel>接收单聊</FormLabel>
                                                            <p className="text-xs text-muted-foreground mt-1">
                                                                允许飞书用户私聊机器人驱动总管家。
                                                            </p>
                                                        </div>
                                                        <FormControl>
                                                            <Switch
                                                                checked={field.value === true || field.value === "true"}
                                                                onCheckedChange={field.onChange}
                                                            />
                                                        </FormControl>
                                                    </FormItem>
                                                )}
                                            />

                                            <Controller
                                                control={form.control}
                                                name="butler_feishu_receive_group"
                                                render={({ field }) => (
                                                    <FormItem className={toggleCardClassName}>
                                                        <div>
                                                            <FormLabel>接收群聊</FormLabel>
                                                            <p className="text-xs text-muted-foreground mt-1">
                                                                允许群消息进入总管家。
                                                            </p>
                                                        </div>
                                                        <FormControl>
                                                            <Switch
                                                                checked={field.value === true || field.value === "true"}
                                                                onCheckedChange={field.onChange}
                                                            />
                                                        </FormControl>
                                                    </FormItem>
                                                )}
                                            />
                                        </div>

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_group_require_mention"
                                            render={({ field }) => (
                                                <FormItem className={toggleCardClassName}>
                                                    <div>
                                                        <FormLabel>@ 或回复后才处理群消息</FormLabel>
                                                        <p className="text-xs text-muted-foreground mt-1">
                                                            群聊默认只处理带 mention 或回复机器人已发送消息的文本。
                                                        </p>
                                                    </div>
                                                    <FormControl>
                                                        <Switch
                                                            checked={field.value === true || field.value === "true"}
                                                            onCheckedChange={field.onChange}
                                                        />
                                                    </FormControl>
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_only_reply_feishu_originated"
                                            render={({ field }) => (
                                                <FormItem className={toggleCardClassName}>
                                                    <div>
                                                        <FormLabel>是否只返回飞书请求的响应到飞书</FormLabel>
                                                        <p className="text-xs text-muted-foreground mt-1">
                                                            默认关闭。关闭时，总管家主会话会尽量同步到 AIPP 与飞书；开启后，仅飞书触发的回合会回发到飞书。
                                                        </p>
                                                    </div>
                                                    <FormControl>
                                                        <Switch
                                                            checked={field.value === true || field.value === "true"}
                                                            onCheckedChange={field.onChange}
                                                        />
                                                    </FormControl>
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_allowed_open_ids"
                                            render={({ field }) => (
                                                <FormItem>
                                                    <FormLabel>允许对话的用户 Open ID 列表</FormLabel>
                                                    <FormControl>
                                                        <Textarea
                                                            {...field}
                                                            value={field.value || ""}
                                                            placeholder={"ou_xxx\nou_yyy"}
                                                            rows={4}
                                                        />
                                                    </FormControl>
                                                    <FormDescription>
                                                        为空则不按用户限制；支持换行、逗号或分号分隔。
                                                    </FormDescription>
                                                    <FormMessage />
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_allowed_chat_ids"
                                            render={({ field }) => (
                                                <FormItem>
                                                    <FormLabel>允许对话的群 Chat ID 列表</FormLabel>
                                                    <FormControl>
                                                        <Textarea
                                                            {...field}
                                                            value={field.value || ""}
                                                            placeholder={"oc_xxx\noc_yyy"}
                                                            rows={4}
                                                        />
                                                    </FormControl>
                                                    <FormDescription>
                                                        仅对群聊生效；为空则不按群限制。
                                                    </FormDescription>
                                                    <FormMessage />
                                                </FormItem>
                                            )}
                                        />
                                    </div>
                                )}
                            </div>
                        )}

                    </div>

                    <div className="pt-4 border-t border-border">
                        <Button
                            type="button"
                            onClick={handleSave}
                            disabled={saveDisabled}
                            className="bg-primary hover:bg-primary/90 text-primary-foreground"
                        >
                            {isSaving ? "保存中..." : "保存配置"}
                        </Button>
                        {showSummarySection && dynamicEnabled && !summarizerModelId && (
                            <p className="text-sm text-destructive mt-2">请先选择 MCP 总结 AI 模型</p>
                        )}
                        {showSummarySection && assistantSummaryEnabled && !assistantSummarizerModelId && (
                            <p className="text-sm text-destructive mt-2">请先选择助手总结 AI 模型</p>
                        )}
                        {showSummarySection && conversationSummaryEnabled && !conversationSummaryModel && (
                            <p className="text-sm text-destructive mt-2">请先选择对话总结模型</p>
                        )}
                        {butlerEnabled && !butlerMainWorkspacePath.trim() && (
                            <p className="text-sm text-destructive mt-2">请先配置主工作区</p>
                        )}
                        {butlerEnabled && feishuEnabled && !feishuAppId.trim() && (
                            <p className="text-sm text-destructive mt-2">请先填写飞书 App ID</p>
                        )}
                        {butlerEnabled && feishuEnabled && !feishuAppSecret.trim() && !feishuStatus?.secret_configured && (
                            <p className="text-sm text-destructive mt-2">请先填写飞书 App Secret</p>
                        )}
                    </div>
                </CardContent>
            </Card>
            {saveFeatureConfigProp && (
                <ButlerOnboardingWizard
                    open={isOnboardingOpen}
                    onOpenChange={setIsOnboardingOpen}
                    existingModelId={butlerModelId}
                    existingDisplayName={String(form.watch("butler_display_name") || "总管家")}
                    existingTrustAll={trustAllWorkspaces}
                    existingMainWorkspace={workspaceConfig.mainWorkspace}
                    existingTrustedWorkspaces={trustedWorkspaces}
                    existingFeishuEnabled={feishuEnabled}
                    existingFeishuAppId={feishuAppId}
                    existingFeishuBaseUrl={feishuBaseUrl}
                    initialValues={form.getValues()}
                    saveFeatureConfig={saveFeatureConfigProp}
                    onComplete={() => {
                        onConfigRefresh?.();
                    }}
                />
            )}
        </Form>
    );
};

export default React.memo(ExperimentalConfigForm);
