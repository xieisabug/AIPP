import React, { useCallback, useEffect, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import ConfigForm from "@/components/ConfigForm";
import { Loader2 } from "lucide-react";

interface OtherConfigFormProps {
    form: UseFormReturn<any>;
}

export const OtherConfigForm: React.FC<OtherConfigFormProps> = ({ form }) => {
    const [systemAutostartEnabled, setSystemAutostartEnabled] = useState<boolean | null>(null);
    const [isToggling, setIsToggling] = useState(false);

    useEffect(() => {
        const loadSystemState = async () => {
            try {
                console.log("[Autostart] invoking get_autostart_state");
                const enabled = await invoke<boolean>("get_autostart_state");
                console.log("[Autostart] get_autostart_state returned:", enabled);
                setSystemAutostartEnabled(enabled);
                form.setValue("autostart_enabled", enabled ? "true" : "false");
            } catch (e) {
                console.error("[Autostart] get_autostart_state failed:", e);
            }
        };
        loadSystemState();
    }, [form]);

    const handleAutostartChange = useCallback(async (value: string | boolean) => {
        const checked = value === true || value === "true";
        console.log("[Autostart] handleAutostartChange called:", { value, checked });
        setIsToggling(true);
        try {
            console.log("[Autostart] invoking set_autostart with:", { enabled: checked });
            await invoke("set_autostart", { enabled: checked });
            console.log("[Autostart] set_autostart succeeded");
            setSystemAutostartEnabled(checked);
            form.setValue("autostart_enabled", checked ? "true" : "false");
            toast.success(checked ? "已开启开机自启动" : "已关闭开机自启动");
        } catch (e) {
            console.error("[Autostart] set_autostart failed:", e);
            toast.error("设置失败: " + e);
            form.setValue("autostart_enabled", systemAutostartEnabled ? "true" : "false");
        } finally {
            setIsToggling(false);
        }
    }, [form, systemAutostartEnabled]);

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
            title="其他配置"
            description="设置应用的其他系统相关配置"
            config={AUTOSTART_FORM_CONFIG}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
        />
    );
};

export default OtherConfigForm;
