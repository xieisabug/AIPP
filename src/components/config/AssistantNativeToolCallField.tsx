import React, { useCallback, useEffect, useState } from 'react';
import { invoke } from "@tauri-apps/api/core";
import { Switch } from "../ui/switch";
import { Info } from "lucide-react";
import { toast } from 'sonner';
import {
    Tooltip,
    TooltipContent,
    TooltipProvider,
    TooltipTrigger,
} from "../ui/tooltip";

interface AssistantNativeToolCallFieldProps {
    assistantId: number;
    onConfigChange?: () => void;
}

const AssistantNativeToolCallField: React.FC<AssistantNativeToolCallFieldProps> = ({
    assistantId,
    onConfigChange
}) => {
    const [useNativeToolCall, setUseNativeToolCall] = useState<boolean>(false);
    const [loading, setLoading] = useState(true);

    const fetchConfig = useCallback(async () => {
        try {
            setLoading(true);
            const nativeToolCallValue = await invoke<string>('get_assistant_field_value', {
                assistantId,
                fieldName: 'use_native_toolcall'
            });
            setUseNativeToolCall(nativeToolCallValue === 'true');
        } catch (error) {
            setUseNativeToolCall(false);
        } finally {
            setLoading(false);
        }
    }, [assistantId]);

    useEffect(() => {
        fetchConfig();
    }, [fetchConfig]);

    const handleToggle = useCallback(async (checked: boolean) => {
        try {
            await invoke('update_assistant_model_config_value', {
                assistantId,
                configName: 'use_native_toolcall',
                configValue: checked.toString(),
                valueType: 'boolean'
            });

            setUseNativeToolCall(checked);
            toast.success(`原生ToolCall已${checked ? '启用' : '禁用'}`);
            onConfigChange?.();
        } catch (error) {
            console.error('Failed to update native toolcall config:', error);
            toast.error('更新原生ToolCall配置失败: ' + error);
        }
    }, [assistantId, onConfigChange]);

    if (loading) {
        return (
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-foreground">使用原生ToolCall</span>
                </div>
                <div className="h-6 w-11 bg-muted rounded-full animate-pulse" />
            </div>
        );
    }

    return (
        <div className="space-y-2">
            <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                    <span className="text-sm font-medium text-foreground">使用原生ToolCall</span>
                    <TooltipProvider>
                        <Tooltip>
                            <TooltipTrigger asChild>
                                <Info className="h-4 w-4 text-muted-foreground cursor-help" />
                            </TooltipTrigger>
                            <TooltipContent>
                                <p className="max-w-xs text-xs">
                                    如果模型支持并且模型能力够强，推荐使用原生Toolcall调用工具更加准确
                                </p>
                            </TooltipContent>
                        </Tooltip>
                    </TooltipProvider>
                </div>
                <Switch
                    checked={useNativeToolCall}
                    onCheckedChange={handleToggle}
                />
            </div>
            <p className="text-xs text-muted-foreground">
                {useNativeToolCall ? '已启用原生ToolCall调用' : '使用prompt方式调用工具'}
            </p>
        </div>
    );
};

export default AssistantNativeToolCallField;
