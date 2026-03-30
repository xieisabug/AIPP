import React from "react";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Bot, Sparkles } from "lucide-react";

interface ModelOption {
    value: string;
    label: string;
}

interface StepModelConfigProps {
    modelId: string;
    displayName: string;
    modelOptions: ModelOption[];
    modelsLoading: boolean;
    onModelChange: (modelId: string) => void;
    onDisplayNameChange: (name: string) => void;
}

const StepModelConfig: React.FC<StepModelConfigProps> = ({
    modelId,
    displayName,
    modelOptions,
    modelsLoading,
    onModelChange,
    onDisplayNameChange,
}) => {
    return (
        <div className="space-y-6">
            <div className="space-y-2">
                <div className="flex items-center gap-2 text-primary">
                    <Sparkles className="h-5 w-5" />
                    <h3 className="text-lg font-semibold">选择总管家的大脑</h3>
                </div>
                <p className="text-sm text-muted-foreground leading-relaxed">
                    总管家使用内置的系统提示词工作，你需要为它选择一个大语言模型。
                    模型的选择会影响总管家的推理能力、响应速度和消耗的 Token 数。
                    推荐使用 Claude 或 GPT-4 级别的模型以获得最佳体验。
                </p>
            </div>

            <div className="space-y-4 rounded-lg border border-border/60 bg-muted/30 p-4">
                <div className="space-y-2">
                    <label className="text-sm font-medium">总管家模型 <span className="text-destructive">*</span></label>
                    <Select
                        value={modelId}
                        onValueChange={onModelChange}
                        disabled={modelsLoading}
                    >
                        <SelectTrigger>
                            <SelectValue
                                placeholder={
                                    modelsLoading ? "加载模型列表..." : "选择一个模型"
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
                    {!modelId && (
                        <p className="text-xs text-destructive">请选择一个模型才能继续</p>
                    )}
                </div>

                <div className="space-y-2">
                    <label className="text-sm font-medium flex items-center gap-2">
                        <Bot className="h-4 w-4" />
                        显示名称
                    </label>
                    <Input
                        value={displayName}
                        onChange={(e) => onDisplayNameChange(e.target.value)}
                        placeholder="总管家"
                    />
                    <p className="text-xs text-muted-foreground">
                        用于工作台头部展示，留空时默认为"总管家"。
                    </p>
                </div>
            </div>
        </div>
    );
};

export default StepModelConfig;
