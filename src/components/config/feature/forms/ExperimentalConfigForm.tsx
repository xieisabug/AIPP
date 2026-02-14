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

export const ExperimentalConfigForm: React.FC<ExperimentalConfigFormProps> = ({ form, onSave }) => {
    const [isSaving, setIsSaving] = useState(false);
    const [summaryProgress, setSummaryProgress] = useState<MCPSummaryProgressPayload | null>(null);

    const dynamicEnabled =
        form.watch("dynamic_mcp_loading_enabled") === true ||
        form.watch("dynamic_mcp_loading_enabled") === "true";
    const summarizerModelId = form.watch("mcp_summarizer_model_id") || "";
    const { models, loading: modelsLoading, error: modelsError } = useModels(dynamicEnabled);

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

    const handleSave = useCallback(async () => {
        if (dynamicEnabled && !summarizerModelId) {
            toast.error("请先选择 MCP 总结 AI 模型后再保存");
            return;
        }

        setIsSaving(true);
        setSummaryProgress(null);
        try {
            await onSave();

            if (dynamicEnabled) {
                await invoke("summarize_all_mcp_catalogs");
                toast.success("实验性配置保存成功，MCP 总结已完成");
            } else {
                toast.success("实验性配置保存成功");
            }
        } catch (error) {
            toast.error("保存实验性配置失败: " + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    }, [dynamicEnabled, onSave, summarizerModelId]);

    const progressValue =
        summaryProgress && summaryProgress.total > 0
            ? Math.round((summaryProgress.completed / summaryProgress.total) * 100)
            : 0;
    const saveDisabled = isSaving || (dynamicEnabled && !summarizerModelId);

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
                        {dynamicEnabled && !summarizerModelId && (
                            <p className="text-sm text-destructive mt-2">请先选择 MCP 总结 AI 模型</p>
                        )}
                    </div>
                </CardContent>
            </Card>
        </Form>
    );
};

export default React.memo(ExperimentalConfigForm);
