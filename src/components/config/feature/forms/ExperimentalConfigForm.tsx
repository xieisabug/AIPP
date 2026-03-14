import React, { useCallback, useEffect, useMemo, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { Controller } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";
import { useModels } from "@/hooks/useModels";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Form, FormControl, FormItem, FormLabel, FormMessage } from "@/components/ui/form";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";

interface ExperimentalConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
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

export const ExperimentalConfigForm: React.FC<ExperimentalConfigFormProps> = ({ form, onSave }) => {
    const [isSaving, setIsSaving] = useState(false);
    const [summaryProgress, setSummaryProgress] = useState<MCPSummaryProgressPayload | null>(null);
    const [assistantSummaryProgress, setAssistantSummaryProgress] =
        useState<AssistantSummaryProgressPayload | null>(null);

    const dynamicEnabled =
        form.watch("dynamic_mcp_loading_enabled") === true ||
        form.watch("dynamic_mcp_loading_enabled") === "true";
    const assistantSummaryEnabled =
        form.watch("assistant_summary_enabled") === true ||
        form.watch("assistant_summary_enabled") === "true";
    const conversationSummaryEnabled =
        form.watch("conversation_summary_enabled") === true ||
        form.watch("conversation_summary_enabled") === "true";
    const butlerEnabled =
        form.watch("butler_experiment_enabled") === true ||
        form.watch("butler_experiment_enabled") === "true";
    const summarizerModelId = form.watch("mcp_summarizer_model_id") || "";
    const assistantSummarizerModelId = form.watch("assistant_summarizer_model_id") || "";
    const conversationSummaryModel = form.watch("conversation_summary_model") || "";
    const butlerModelId = form.watch("butler_model_id") || "";
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
    useEffect(() => {
        const unlistenPromise = listen<MCPSummaryProgressPayload>("mcp-summary-progress", (event) => {
            setSummaryProgress(event.payload);
        });
        return () => {
            unlistenPromise.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, []);

    useEffect(() => {
        const unlistenPromise = listen<AssistantSummaryProgressPayload>(
            "assistant-summary-progress",
            (event) => {
                setAssistantSummaryProgress(event.payload);
            }
        );
        return () => {
            unlistenPromise.then((unlisten) => unlisten()).catch(console.warn);
        };
    }, []);

    const runAssistantSummary = useCallback(async (showSuccessToast: boolean) => {
        setAssistantSummaryProgress({
            phase: "started",
            total: 0,
            completed: 0,
            succeeded: 0,
            failed: 0,
            assistant_name: undefined,
            message: "正在启动助手画像生成...",
        });
        await invoke("summarize_all_assistant_summaries");
        if (showSuccessToast) {
            toast.success("助手画像生成完成");
        }
    }, []);

    const handleSave = useCallback(async () => {
        if (dynamicEnabled && !summarizerModelId) {
            toast.error("请先选择 MCP 总结 AI 模型后再保存");
            return;
        }
        if (assistantSummaryEnabled && !assistantSummarizerModelId) {
            toast.error("请先选择助手总结 AI 模型后再保存");
            return;
        }
        if (conversationSummaryEnabled && !conversationSummaryModel) {
            toast.error("请先选择对话总结模型后再保存");
            return;
        }
        if (butlerEnabled && !butlerModelId) {
            toast.error("请先为总管家模式选择模型");
            return;
        }

        setIsSaving(true);
        setSummaryProgress(null);
        setAssistantSummaryProgress(null);
        try {
            await onSave();

            if (dynamicEnabled) {
                await invoke("summarize_all_mcp_catalogs");
            }
            if (assistantSummaryEnabled) {
                await runAssistantSummary(false);
            }
            toast.success("实验性配置保存成功");
        } catch (error) {
            toast.error("保存实验性配置失败: " + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    }, [
        assistantSummarizerModelId,
        assistantSummaryEnabled,
        butlerEnabled,
        butlerModelId,
        conversationSummaryEnabled,
        conversationSummaryModel,
        dynamicEnabled,
        onSave,
        runAssistantSummary,
        summarizerModelId,
    ]);

    const handleRunAssistantSummary = useCallback(async () => {
        if (!assistantSummarizerModelId) {
            toast.error("请先选择助手总结 AI 模型");
            return;
        }
        setIsSaving(true);
        try {
            await runAssistantSummary(true);
        } catch (error) {
            toast.error("生成助手画像失败: " + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    }, [assistantSummarizerModelId, runAssistantSummary]);

    const progressValue =
        summaryProgress && summaryProgress.total > 0
            ? Math.round((summaryProgress.completed / summaryProgress.total) * 100)
            : 0;
    const assistantSummaryProgressValue =
        assistantSummaryProgress && assistantSummaryProgress.total > 0
            ? Math.round((assistantSummaryProgress.completed / assistantSummaryProgress.total) * 100)
            : 0;
    const saveDisabled =
        isSaving
        || (dynamicEnabled && !summarizerModelId)
        || (assistantSummaryEnabled && !assistantSummarizerModelId)
        || (conversationSummaryEnabled && !conversationSummaryModel)
        || (butlerEnabled && !butlerModelId);

    return (
        <Form {...form}>
            <Card className="shadow-none border-l-4 border-l-primary bottom-space">
                <CardHeader>
                    <CardTitle className="text-lg font-semibold">实验性功能</CardTitle>
                    <p className="text-sm text-muted-foreground">新能力可能存在兼容风险，请按需启用。</p>
                </CardHeader>
                <CardContent className="space-y-6">
                    <Controller
                        control={form.control}
                        name="butler_experiment_enabled"
                        render={({ field }) => (
                            <FormItem className="flex items-center justify-between rounded-md border p-4">
                                <div>
                                    <FormLabel className="text-base">总管家模式（实验）</FormLabel>
                                    <p className="text-sm text-muted-foreground mt-1">
                                        开启后新增独立的总管家工作台、隐藏任务会话和任务结果聚合视图。
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
                        name="dynamic_mcp_loading_enabled"
                        render={({ field }) => (
                            <FormItem className="flex items-center justify-between rounded-md border p-4">
                                <div>
                                    <FormLabel className="text-base">MCP 动态加载（实验）</FormLabel>
                                    <p className="text-sm text-muted-foreground mt-1">
                                        开启后采用 MCP 目录摘要 + 按需加载模式。
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

                    {dynamicEnabled && (
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
                                    <p className="text-sm text-muted-foreground mt-1">
                                        保存后将顺序总结所有已启用 MCP Server，并实时更新进度。
                                    </p>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />
                    )}

                    <Controller
                        control={form.control}
                        name="assistant_summary_enabled"
                        render={({ field }) => (
                            <FormItem className="flex items-center justify-between rounded-md border p-4">
                                <div>
                                    <FormLabel className="text-base">助手总结（实验）</FormLabel>
                                    <p className="text-sm text-muted-foreground mt-1">
                                        根据助手名称、提示词、MCP 和 Skills 生成执行画像，并注入到总管家 prompt。
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

                    {assistantSummaryEnabled && (
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
                                    <p className="text-sm text-muted-foreground mt-1">
                                        保存后会顺序总结所有普通助手，并把可派发助手画像注入总管家系统提示词。
                                    </p>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />
                    )}

                    {assistantSummaryEnabled && assistantSummaryProgress && (
                        <div className="space-y-2 rounded-md border p-4 bg-muted/30">
                            <div className="flex items-center justify-between text-sm">
                                <span className="font-medium">正在总结助手画像</span>
                                <span>
                                    {assistantSummaryProgress.total > 0
                                        ? `${assistantSummaryProgress.completed}/${assistantSummaryProgress.total}`
                                        : "启动中"}
                                </span>
                            </div>
                            <Progress value={assistantSummaryProgressValue} />
                            <p className="text-xs text-muted-foreground">
                                {assistantSummaryProgress.phase === "processing"
                                    ? `当前: ${assistantSummaryProgress.assistant_name || "处理中"}`
                                    : assistantSummaryProgress.phase === "completed"
                                        ? `完成: 成功 ${assistantSummaryProgress.succeeded}，失败 ${assistantSummaryProgress.failed}`
                                        : assistantSummaryProgress.message || "准备中..."}
                            </p>
                        </div>
                    )}

                    <Controller
                        control={form.control}
                        name="conversation_summary_enabled"
                        render={({ field }) => (
                            <FormItem className="flex items-center justify-between rounded-md border p-4">
                                <div>
                                    <FormLabel className="text-base">对话总结（实验）</FormLabel>
                                    <p className="text-sm text-muted-foreground mt-1">
                                        对话空闲一段时间后自动生成摘要，用于搜索、回顾和后续上下文重建。
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

                    {conversationSummaryEnabled && (
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
                                    <p className="text-sm text-muted-foreground mt-1">
                                        该配置会优先覆盖旧的“辅助AI → 对话总结”设置。
                                    </p>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />
                    )}

                    {butlerEnabled && (
                        <div className="space-y-4 rounded-md border p-4">
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
                        </div>
                    )}

                    {dynamicEnabled && summaryProgress && (
                        <div className="space-y-2 rounded-md border p-4 bg-muted/30">
                            <div className="flex items-center justify-between text-sm">
                                <span className="font-medium">正在总结 MCP</span>
                                <span>
                                    {summaryProgress.completed}/{summaryProgress.total}
                                </span>
                            </div>
                            <Progress value={progressValue} />
                            <p className="text-xs text-muted-foreground">
                                {summaryProgress.phase === "processing"
                                    ? `当前: ${summaryProgress.server_name || "处理中"}`
                                    : summaryProgress.phase === "completed"
                                        ? `完成: 成功 ${summaryProgress.succeeded}，失败 ${summaryProgress.failed}`
                                        : summaryProgress.message || "准备中..."}
                            </p>
                        </div>
                    )}

                    <div className="pt-4 border-t border-border">
                        <Button
                            type="button"
                            onClick={handleSave}
                            disabled={saveDisabled}
                            className="bg-primary hover:bg-primary/90 text-primary-foreground"
                        >
                            {isSaving ? "保存中..." : "保存配置"}
                        </Button>
                        {assistantSummaryEnabled && (
                            <Button
                                type="button"
                                variant="outline"
                                onClick={handleRunAssistantSummary}
                                disabled={isSaving || !assistantSummarizerModelId}
                                className="ml-2"
                            >
                                立即生成助手画像
                            </Button>
                        )}
                        {dynamicEnabled && !summarizerModelId && (
                            <p className="text-sm text-destructive mt-2">请先选择 MCP 总结 AI 模型</p>
                        )}
                        {assistantSummaryEnabled && !assistantSummarizerModelId && (
                            <p className="text-sm text-destructive mt-2">请先选择助手总结 AI 模型</p>
                        )}
                        {conversationSummaryEnabled && !conversationSummaryModel && (
                            <p className="text-sm text-destructive mt-2">请先选择对话总结模型</p>
                        )}
                    </div>
                </CardContent>
            </Card>
        </Form>
    );
};

export default React.memo(ExperimentalConfigForm);
