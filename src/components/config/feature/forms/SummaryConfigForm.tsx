import React, { useCallback, useMemo } from "react";
import { UseFormReturn } from "react-hook-form";
import { Controller } from "react-hook-form";
import { ChevronDown, ChevronRight } from "lucide-react";
import { toast } from "sonner";
import { getErrorMessage } from "@/utils/error";
import { useModels } from "@/hooks/useModels";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Form, FormItem, FormLabel, FormControl, FormDescription, FormMessage } from "@/components/ui/form";
import { Textarea } from "@/components/ui/textarea";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";

interface SummaryConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
}

// 可折叠的配置分组
interface ConfigSectionProps {
    title: string;
    description?: string;
    enabled: boolean;
    onEnabledChange: (value: boolean) => void;
    children: React.ReactNode;
}

const ConfigSection: React.FC<ConfigSectionProps> = ({
    title,
    description,
    enabled,
    onEnabledChange,
    children,
}) => {
    const [isExpanded, setIsExpanded] = React.useState(true);

    return (
        <Card className="border-l-4 border-l-muted shadow-sm">
            <CardHeader
                className="flex flex-row items-center py-3 cursor-pointer hover:bg-muted/50 transition-colors rounded-t-lg"
                onClick={() => setIsExpanded(!isExpanded)}
            >
                <div className="flex items-center flex-1">
                    <div className="mr-2 text-muted-foreground">
                        {isExpanded ? <ChevronDown className="h-4 w-4" /> : <ChevronRight className="h-4 w-4" />}
                    </div>
                    <div className="flex-1">
                        <CardTitle className="text-base font-medium">{title}</CardTitle>
                        {description && (
                            <p className="text-sm text-muted-foreground mt-0.5">{description}</p>
                        )}
                    </div>
                </div>
                <div
                    className="flex items-center gap-2"
                    onClick={(e) => e.stopPropagation()}
                >
                    <span className="text-sm text-muted-foreground mr-2">
                        {enabled ? "启用" : "禁用"}
                    </span>
                    <Switch
                        checked={enabled}
                        onCheckedChange={onEnabledChange}
                    />
                </div>
            </CardHeader>
            {isExpanded && (
                <CardContent className="pt-2 pb-4">
                    <div className={enabled ? "opacity-100" : "opacity-50 pointer-events-none"}>
                        {children}
                    </div>
                </CardContent>
            )}
        </Card>
    );
};

// 模型选择组件
interface ModelSelectProps {
    value: string;
    onChange: (value: string) => void;
    placeholder?: string;
    disabled?: boolean;
}

const ModelSelect: React.FC<ModelSelectProps> = ({
    value,
    onChange,
    placeholder = "选择模型",
    disabled,
}) => {
    const { models, loading, error } = useModels(!disabled);

    const modelOptions = useMemo(() => {
        return models.map((model) => ({
            value: `${model.code}%%${model.llm_provider_id}`,
            label: model.name,
        }));
    }, [models]);

    return (
        <Select
            disabled={disabled || loading}
            value={value}
            onValueChange={onChange}
        >
            <SelectTrigger className="w-full">
                <SelectValue
                    placeholder={
                        loading ? "加载中..." : error ? "加载失败" : placeholder
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
    );
};

export const SummaryConfigForm: React.FC<SummaryConfigFormProps> = ({
    form,
    onSave,
}) => {
    const handleSaveSummary = useCallback(async () => {
        try {
            await onSave();
            toast.success("辅助AI配置保存成功");
        } catch (e) {
            toast.error("保存辅助AI配置失败: " + getErrorMessage(e));
        }
    }, [form, onSave]);

    const summaryLengthOptions = [50, 100, 300, 500, 1000, -1].map((m) => ({
        value: m.toString(),
        label: m === -1 ? "所有" : m.toString(),
    }));

    return (
        <Form {...form}>
            <div className="space-y-4">
                {/* 总开关 */}
                <Card className="border-l-4 border-l-primary">
                    <CardContent className="py-4">
                        <div className="flex items-center justify-between">
                            <div>
                                <h3 className="font-medium text-base">启用辅助AI</h3>
                                <p className="text-sm text-muted-foreground">
                                    关闭后将禁用所有AI辅助功能
                                </p>
                            </div>
                            <Controller
                                control={form.control}
                                name="assistant_ai_enabled"
                                render={({ field }) => (
                                    <Switch
                                        checked={field.value === true || field.value === "true"}
                                        onCheckedChange={field.onChange}
                                    />
                                )}
                            />
                        </div>
                    </CardContent>
                </Card>

                {/* 总开关关闭时显示提示 */}
                {!form.watch("assistant_ai_enabled") && (
                    <div className="text-sm text-muted-foreground bg-muted p-3 rounded-md">
                        辅助AI功能已禁用，请开启总开关以配置各项功能
                    </div>
                )}

                {/* 总结标题 */}
                <ConfigSection
                    title="总结标题"
                    description="对话开始时总结该对话并生成标题"
                    enabled={form.watch("title_summary_enabled") === true || form.watch("title_summary_enabled") === "true"}
                    onEnabledChange={(value) => form.setValue("title_summary_enabled", value)}
                >
                    <div className="space-y-4">
                        <Controller
                            control={form.control}
                            name="title_model"
                            render={({ field }) => (
                                <FormItem>
                                    <FormLabel>总结模型</FormLabel>
                                    <FormControl>
                                        <ModelSelect
                                            value={field.value || ""}
                                            onChange={field.onChange}
                                            placeholder="选择总结模型"
                                            disabled={!form.watch("title_summary_enabled")}
                                        />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <Controller
                            control={form.control}
                            name="title_summary_length"
                            render={({ field }) => (
                                <FormItem>
                                    <FormLabel>总结文本长度</FormLabel>
                                    <FormControl>
                                        <Select
                                            disabled={!form.watch("title_summary_enabled")}
                                            value={field.value}
                                            onValueChange={field.onChange}
                                        >
                                            <SelectTrigger>
                                                <SelectValue placeholder="选择长度" />
                                            </SelectTrigger>
                                            <SelectContent>
                                                {summaryLengthOptions.map((option) => (
                                                    <SelectItem key={option.value} value={option.value}>
                                                        {option.label}
                                                    </SelectItem>
                                                ))}
                                            </SelectContent>
                                        </Select>
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />

                        <Controller
                            control={form.control}
                            name="title_prompt"
                            render={({ field }) => (
                                <FormItem>
                                    <FormLabel>总结 Prompt</FormLabel>
                                    <FormControl>
                                        <Textarea
                                            className="h-40"
                                            disabled={!form.watch("title_summary_enabled")}
                                            {...field}
                                        />
                                    </FormControl>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />
                    </div>
                </ConfigSection>

                {/* 表单自动填写 */}
                <ConfigSection
                    title="表单自动填写"
                    description="根据对话内容自动填写表单字段"
                    enabled={form.watch("form_autofill_enabled") === true || form.watch("form_autofill_enabled") === "true"}
                    onEnabledChange={(value) => form.setValue("form_autofill_enabled", value)}
                >
                    <Controller
                        control={form.control}
                        name="form_autofill_model"
                        render={({ field }) => (
                            <FormItem>
                                <FormLabel>表单填写模型</FormLabel>
                                <FormControl>
                                    <ModelSelect
                                        value={field.value || ""}
                                        onChange={field.onChange}
                                        placeholder="选择表单填写模型"
                                        disabled={!form.watch("form_autofill_enabled")}
                                    />
                                </FormControl>
                                <FormMessage />
                            </FormItem>
                        )}
                    />
                </ConfigSection>

                {/* 记忆总结 */}
                {/* <ConfigSection
                    title="记忆总结（实验）"
                    description="提取关键信息生成记忆"
                    enabled={form.watch("memory_summary_enabled") === true || form.watch("memory_summary_enabled") === "true"}
                    onEnabledChange={(value) => form.setValue("memory_summary_enabled", value)}
                >
                    <Controller
                        control={form.control}
                        name="memory_summary_model"
                        render={({ field }) => (
                            <FormItem>
                                <FormLabel>记忆生成模型</FormLabel>
                                <FormControl>
                                    <ModelSelect
                                        value={field.value || ""}
                                        onChange={field.onChange}
                                        placeholder="选择记忆生成模型"
                                        disabled={!form.watch("memory_summary_enabled")}
                                    />
                                </FormControl>
                                <FormMessage />
                            </FormItem>
                        )}
                    />
                </ConfigSection> */}

                <ConfigSection
                    title="上下文压缩"
                    description="为长对话自动压缩较早历史，适用于使用普通聊天链路的对话"
                    enabled={
                        form.watch("context_compaction_enabled") === true
                        || form.watch("context_compaction_enabled") === "true"
                    }
                    onEnabledChange={(value) => form.setValue("context_compaction_enabled", value)}
                >
                    <div className="space-y-4">
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
                                            disabled={!form.watch("context_compaction_enabled")}
                                            placeholder="128000"
                                        />
                                    </FormControl>
                                    <FormDescription>
                                        模型的上下文窗口总大小（含输入与输出），系统会自动预留输出空间。
                                    </FormDescription>
                                    <FormMessage />
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
                                            disabled={!form.watch("context_compaction_enabled")}
                                            placeholder="0.80"
                                        />
                                    </FormControl>
                                    <FormDescription>
                                        当估算 Token 使用量达到此比例时触发自动压缩（0.5 ~ 0.99）。
                                    </FormDescription>
                                    <FormMessage />
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
                                            disabled={!form.watch("context_compaction_enabled")}
                                            placeholder="0.30"
                                        />
                                    </FormControl>
                                    <FormDescription>
                                        为最近消息保留的上下文预算比例。系统会按 Token 从最新消息往前自动决定保留范围。
                                    </FormDescription>
                                    <FormMessage />
                                </FormItem>
                            )}
                        />
                    </div>
                </ConfigSection>

                {/* 保存按钮 */}
                <div className="pt-4 border-t border-border">
                    <Button onClick={handleSaveSummary} className="bg-primary hover:bg-primary/90 text-primary-foreground">
                        保存配置
                    </Button>
                </div>
            </div>
        </Form>
    );
};
