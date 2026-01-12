import React, { useCallback, useEffect, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import ConfigForm from "@/components/ConfigForm";
import { Loader2 } from "lucide-react";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";

interface OtherConfigFormProps {
    form: UseFormReturn<any>;
}

export const OtherConfigForm: React.FC<OtherConfigFormProps> = ({ form }) => {
    const [systemAutostartEnabled, setSystemAutostartEnabled] = useState<boolean | null>(null);
    const [isToggling, setIsToggling] = useState(false);

    // 防泄露模式配置
    const { getConfigValue, saveFeatureConfig, loading: featureConfigLoading } = useFeatureConfig();
    const [antiLeakageEnabled, setAntiLeakageEnabled] = useState<boolean>(false);
    const [isTogglingAntiLeakage, setIsTogglingAntiLeakage] = useState(false);

    // 加载防泄露模式配置
    useEffect(() => {
        if (!featureConfigLoading) {
            const enabled = getConfigValue("anti_leakage", "enabled") === "true";
            setAntiLeakageEnabled(enabled);
            form.setValue("anti_leakage_enabled", enabled ? "true" : "false");
        }
    }, [featureConfigLoading, getConfigValue, form]);

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

    const handleAntiLeakageChange = useCallback(async (value: string | boolean) => {
        const checked = value === true || value === "true";
        setIsTogglingAntiLeakage(true);
        try {
            await saveFeatureConfig("anti_leakage", { enabled: checked ? "true" : "false" });
            setAntiLeakageEnabled(checked);
            form.setValue("anti_leakage_enabled", checked ? "true" : "false");
            toast.success(checked ? "已开启防泄露模式" : "已关闭防泄露模式");
        } catch (e) {
            console.error("[AntiLeakage] save_feature_config failed:", e);
            toast.error("设置失败: " + e);
            form.setValue("anti_leakage_enabled", antiLeakageEnabled ? "true" : "false");
        } finally {
            setIsTogglingAntiLeakage(false);
        }
    }, [form, antiLeakageEnabled, saveFeatureConfig]);

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
        {
            key: "anti_leakage_enabled",
            config: {
                type: "switch" as const,
                label: "防泄露模式",
                tooltip: "开启后对话标题和内容将被脱敏处理，保护隐私",
                onChange: handleAntiLeakageChange,
                disabled: isTogglingAntiLeakage || featureConfigLoading,
            },
        },
    ];

    if (systemAutostartEnabled === null || featureConfigLoading) {
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
