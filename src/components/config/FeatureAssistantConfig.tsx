import React, { useCallback, useState, useMemo, useEffect } from "react";
import { MessageSquare, Eye, FolderOpen, Settings, Wifi, Monitor, Keyboard, Power, Info } from "lucide-react";
import { useForm } from "react-hook-form";

// 导入公共组件
import { ConfigPageLayout, SidebarList, ListItemButton, SelectOption } from "../common";

// 导入新的 hooks 和组件
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useVersionManager } from "@/hooks/feature/useVersionManager";
import { FeatureFormRenderer } from "./feature/FeatureFormRenderer";

interface FeatureItem {
    id: string;
    name: string;
    description: string;
    icon: React.ReactNode;
    code: string;
}

interface FeatureItem {
    id: string;
    name: string;
    description: string;
    icon: React.ReactNode;
    code: string;
}

const FeatureAssistantConfig: React.FC = () => {
    // 功能列表定义
    const featureList: FeatureItem[] = [
        {
            id: "display",
            name: "显示",
            description: "配置系统外观主题、深浅色模式和用户消息渲染方式",
            icon: <Monitor className="h-5 w-5" />,
            code: "display",
        },
        {
            id: "conversation_summary",
            name: "AI总结",
            description: "对话开始时总结该对话并且生成标题，表单自动填写",
            icon: <MessageSquare className="h-5 w-5" />,
            code: "conversation_summary",
        },
        {
            id: "preview",
            name: "预览配置",
            description: "在大模型编写完react或者vue组件之后，能够快速预览",
            icon: <Eye className="h-5 w-5" />,
            code: "preview",
        },
        {
            id: "data_folder",
            name: "数据目录",
            description: "管理和同步数据文件夹",
            icon: <FolderOpen className="h-5 w-5" />,
            code: "data_folder",
        },
        {
            id: "network_config",
            name: "网络配置",
            description: "配置请求超时、重试次数和网络代理",
            icon: <Wifi className="h-5 w-5" />,
            code: "network_config",
        },
        {
            id: "shortcuts",
            name: "快捷键",
            description: "配置呼出窗口的快捷方式和回退组合键",
            icon: <Keyboard className="h-5 w-5" />,
            code: "shortcuts",
        },
        {
            id: "other",
            name: "其他",
            description: "系统的其他功能",
            icon: <Power className="h-5 w-5" />,
            code: "other",
        },
        {
            id: "about",
            name: "关于",
            description: "查看应用信息和检查更新",
            icon: <Info className="h-5 w-5" />,
            code: "about",
        },
    ];

    const [selectedFeature, setSelectedFeature] = useState<FeatureItem>(featureList[0]);

    // 使用新的 hooks
    const { featureConfig, saveFeatureConfig, loading } = useFeatureConfig();
    const versionManager = useVersionManager();

    // 初始化表单
    const displayForm = useForm({
        defaultValues: {
            theme: "default",
            color_mode: "system",
            user_message_markdown_render: "disabled",
            notification_on_completion: "false",
            code_theme_light: "github",
            code_theme_dark: "github-dark",
        },
    });

    const summaryForm = useForm({
        defaultValues: {
            model: "",
            summary_length: "100",
            form_autofill_model: "",
            prompt: "",
        },
    });

    const previewForm = useForm({
        defaultValues: {
            preview_type: "service",
            nextjs_port: "3001",
            nuxtjs_port: "3002",
            auth_token: "",
        },
    });

    const networkForm = useForm({
        defaultValues: {
            request_timeout: "180",
            retry_attempts: "3",
            network_proxy: "",
        },
    });

    const dataFolderForm = useForm({});

    // 根据平台设置快捷键默认值
    const isMac = typeof navigator !== 'undefined' && navigator.userAgent.toLowerCase().indexOf('mac') !== -1;
    const shortcutsForm = useForm({
        defaultValues: {
            // 新格式：标准 accelerator 字符串，例如 "Alt+Space"、"Ctrl+Shift+I"
            shortcut: isMac ? "Option+Space" : "Alt+Space",
            // 兼容旧字段
            modifier_key: isMac ? "option" : "alt",
        },
    });

    const otherForm = useForm({
        defaultValues: {
            autostart_enabled: "false",
        },
    });

    const aboutForm = useForm({
        defaultValues: {},
    });

    // 监听 featureConfig 变化，更新表单值
    useEffect(() => {
        if (!loading && featureConfig.size > 0) {
            console.log("feature config loaded", featureConfig);

            // 更新 display 表单
            const displayConfig = featureConfig.get("display");
            if (displayConfig) {
                displayForm.reset({
                    theme: displayConfig.get("theme") || "default",
                    color_mode: displayConfig.get("color_mode") || "system",
                    user_message_markdown_render: displayConfig.get("user_message_markdown_render") || "disabled",
                    notification_on_completion: displayConfig.get("notification_on_completion") || "false",
                    code_theme_light: displayConfig.get("code_theme_light") || "github",
                    code_theme_dark: displayConfig.get("code_theme_dark") || "github-dark",
                });
            }

            // 更新 summary 表单
            const summaryConfig = featureConfig.get("conversation_summary");
            if (summaryConfig) {
                const providerId = summaryConfig.get("provider_id") || "";
                const modelCode = summaryConfig.get("model_code") || "";
                summaryForm.reset({
                    // ConfigForm's model-select uses value format: `${model.code}%%${model.llm_provider_id}`
                    // Keep the same order here when restoring the form value
                    model: `${modelCode}%%${providerId}`,
                    summary_length: summaryConfig.get("summary_length") || "100",
                    form_autofill_model: summaryConfig.get("form_autofill_model") || "",
                    prompt: summaryConfig.get("prompt") || "",
                });
            }

            // 更新 preview 表单
            const previewConfig = featureConfig.get("preview");
            if (previewConfig) {
                previewForm.reset({
                    preview_type: previewConfig.get("preview_type") || "service",
                    nextjs_port: previewConfig.get("nextjs_port") || "3001",
                    nuxtjs_port: previewConfig.get("nuxtjs_port") || "3002",
                    auth_token: previewConfig.get("auth_token") || "",
                });
            }

            // 更新 network 表单
            const networkConfig = featureConfig.get("network_config");
            if (networkConfig) {
                networkForm.reset({
                    request_timeout: networkConfig.get("request_timeout") || "180",
                    retry_attempts: networkConfig.get("retry_attempts") || "3",
                    network_proxy: networkConfig.get("network_proxy") || "",
                });
            }

            // 更新 shortcuts 表单
            const shortcutsConfig = featureConfig.get("shortcuts");
            if (shortcutsConfig) {
                const shortcut = shortcutsConfig.get("shortcut");
                const modifier_key = shortcutsConfig.get("modifier_key") || (isMac ? "option" : "alt");
                // 若无新字段，则按旧逻辑回退到 修饰键+Space
                const fallbackShortcut = (() => {
                    const mk = (modifier_key || "").toLowerCase();
                    if (mk === "ctrl" || mk === "control") return "Ctrl+Space";
                    if (mk === "shift") return "Shift+Space";
                    if (mk === "cmd" || mk === "command" || mk === "super") return isMac ? "Command+Space" : "Super+Space";
                    return isMac ? "Option+Space" : "Alt+Space";
                })();
                shortcutsForm.reset({
                    shortcut: shortcut || fallbackShortcut,
                    modifier_key,
                });
            }

            // 更新 autostart 表单
            const autostartConfig = featureConfig.get("autostart");
            if (autostartConfig) {
                const enabled = autostartConfig.get("autostart_enabled") || "false";
                otherForm.reset({
                    autostart_enabled: enabled,
                });
            }
        }
    }, [loading, featureConfig, displayForm, summaryForm, previewForm, networkForm, shortcutsForm, otherForm]);

    // 选择功能
    const handleSelectFeature = useCallback((feature: FeatureItem) => {
        setSelectedFeature(feature);
    }, []);

    // 保存功能配置的回调函数
    const handleSaveDisplayConfig = useCallback(async () => {
        const values = displayForm.getValues();
        await saveFeatureConfig("display", {
            theme: values.theme,
            color_mode: values.color_mode,
            user_message_markdown_render: values.user_message_markdown_render,
            notification_on_completion: values.notification_on_completion.toString(),
            code_theme_light: values.code_theme_light,
            code_theme_dark: values.code_theme_dark,
        });
    }, [displayForm, saveFeatureConfig]);

    const handleSaveSummaryConfig = useCallback(async () => {
        const values = summaryForm.getValues();
        // model-select value format is `${model_code}%%${provider_id}`
        const [model_code, provider_id] = (values.model as string).split("%%");
        // Ensure provider_id is numeric string
        if (!provider_id || isNaN(Number(provider_id))) {
            throw new Error("provider_id 必须是数字，请重新选择模型");
        }
        await saveFeatureConfig("conversation_summary", {
            provider_id,
            model_code,
            summary_length: values.summary_length,
            form_autofill_model: values.form_autofill_model,
            prompt: values.prompt,
        });
    }, [summaryForm, saveFeatureConfig]);

    const handleSaveNetworkConfig = useCallback(async () => {
        const values = networkForm.getValues();
        await saveFeatureConfig("network_config", {
            request_timeout: values.request_timeout,
            retry_attempts: values.retry_attempts,
            network_proxy: values.network_proxy,
        });
    }, [networkForm, saveFeatureConfig]);

    const handleSaveShortcutsConfig = useCallback(async () => {
        const v = shortcutsForm.getValues();
        await saveFeatureConfig("shortcuts", {
            // 保存新旧两种字段，后端优先读取 shortcut
            shortcut: v.shortcut,
            modifier_key: v.modifier_key,
        });
    }, [shortcutsForm, saveFeatureConfig]);

    // 下拉菜单选项
    const selectOptions: SelectOption[] = useMemo(
        () =>
            featureList.map((feature) => ({
                id: feature.id,
                label: feature.name,
                icon: feature.icon,
            })),
        []
    );

    // 下拉菜单选择回调
    const handleSelectFromDropdown = useCallback(
        (featureId: string) => {
            const feature = featureList.find((f) => f.id === featureId);
            if (feature) {
                handleSelectFeature(feature);
            }
        },
        [handleSelectFeature]
    );

    // 侧边栏内容 - 使用 useMemo 避免重复创建
    const sidebar = useMemo(() => (
        <SidebarList title="程序功能" description="选择功能进行配置" icon={<Settings className="h-5 w-5" />}>
            {featureList.map((feature) => {
                return (
                    <ListItemButton
                        key={feature.id}
                        isSelected={selectedFeature.id === feature.id}
                        onClick={() => handleSelectFeature(feature)}
                    >
                        <div className="flex items-center w-full">
                            <div className="flex-1 flex items-center">
                                {feature.icon}
                                <div className="ml-3 flex-1 truncate">
                                    <div className="font-medium truncate">{feature.name}</div>
                                </div>
                            </div>
                        </div>
                    </ListItemButton>
                );
            })}
        </SidebarList>
    ), [featureList, selectedFeature.id, handleSelectFeature]);

    // 右侧内容 - 使用 useMemo 避免重复创建
    const content = useMemo(() => (
        <div className="space-y-6">
            <FeatureFormRenderer
                selectedFeature={selectedFeature}
                forms={{
                    displayForm,
                    summaryForm,
                    previewForm,
                    networkForm,
                    dataFolderForm,
                    shortcutsForm,
                    otherForm,
                    aboutForm,
                }}
                versionManager={versionManager}
                onSaveDisplay={handleSaveDisplayConfig}
                onSaveSummary={handleSaveSummaryConfig}
                onSaveNetwork={handleSaveNetworkConfig}
                onSaveShortcuts={handleSaveShortcutsConfig}
            />
        </div>
    ), [selectedFeature, displayForm, summaryForm, previewForm, networkForm, dataFolderForm, shortcutsForm, otherForm, aboutForm, versionManager, handleSaveDisplayConfig, handleSaveSummaryConfig, handleSaveNetworkConfig, handleSaveShortcutsConfig]);

    return (
        <ConfigPageLayout
            sidebar={sidebar}
            content={content}
            selectOptions={selectOptions}
            selectedOptionId={selectedFeature.id}
            onSelectOption={handleSelectFromDropdown}
            selectPlaceholder="选择功能"
        />
    );
};

export default FeatureAssistantConfig;
