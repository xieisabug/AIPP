import React, { useCallback, useEffect, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import ConfigForm from "@/components/ConfigForm";
import { Loader2 } from "lucide-react";

interface AutostartConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
}

export const AutostartConfigForm: React.FC<AutostartConfigFormProps> = ({ form, onSave }) => {
    const [systemAutostartEnabled, setSystemAutostartEnabled] = useState<boolean | null>(null);
    const [isToggling, setIsToggling] = useState(false);

    useEffect(() => {
        const loadSystemState = async () => {
            try {
                const enabled = await invoke<boolean>("get_autostart_state");
                setSystemAutostartEnabled(enabled);
                form.setValue("autostart_enabled", enabled ? "true" : "false");
            } catch (e) {
                console.error("Failed to get autostart state:", e);
            }
        };
        loadSystemState();
    }, [form]);

    const handleAutostartChange = useCallback(async (value: string | boolean) => {
        const checked = value === true || value === "true";
        setIsToggling(true);
        try {
            await invoke("set_autostart", { enabled: checked });
            setSystemAutostartEnabled(checked);
            form.setValue("autostart_enabled", checked ? "true" : "false");
            toast.success(checked ? "已开启开机自启动" : "已关闭开机自启动");
        } catch (e) {
            toast.error("设置失败: " + e);
            form.setValue("autostart_enabled", systemAutostartEnabled ? "true" : "false");
        } finally {
            setIsToggling(false);
        }
    }, [form, systemAutostartEnabled]);

    const handleSave = useCallback(async () => {
        await onSave();
        toast.success("配置保存成功");
    }, [onSave]);

    const AUTOSTART_FORM_CONFIG = [
        {
            key: "autostart_enabled",
            config: {
                type: "switch" as const,
                label: "开机自启动",
                tooltip: "应用将在系统启动时自动运行",
                onChange: handleAutostartChange,
                disabled: isToggling || systemAutostartEnabled === null,
            },
        },
    ];

    if (systemAutostartEnabled === null) {
        return (
            <div className="flex items-center justify-center py-12">
                <Loader2 className="h-6 w-6 animate-spin text-muted-foreground" />
                <span className="ml-2 text-sm text-muted-foreground">加载中...</span>
            </div>
        );
    }

    return (
        <ConfigForm
            title="开机自启动"
            description="设置应用在系统启动时自动运行"
            config={AUTOSTART_FORM_CONFIG}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
            onSave={handleSave}
        />
    );
};

export default AutostartConfigForm;
