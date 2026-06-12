import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { UseFormReturn } from "react-hook-form";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { emit, listen } from "@tauri-apps/api/event";
import ConfigForm from "@/components/ConfigForm";
import { toast } from "sonner";
import { AVAILABLE_CODE_THEMES } from "@/hooks/useCodeTheme";
import { useSyntectThemes } from "@/hooks/highlight/useSyntectThemes";
import { pluginRuntime } from "@/services/PluginRuntime";
import { ensureBuiltinMcpToolComponentsRegistered } from "@/services/builtinMcpToolComponents";
import {
    AUTO_MCP_TOOL_COMPONENT_ID,
    DEFAULT_MCP_TOOL_COMPONENT_ID,
    useMcpToolComponentRegistrySnapshot,
} from "@/services/mcpToolComponentRegistry";

interface DisplayConfigFormProps {
    form: UseFormReturn<any>;
    onSave: () => Promise<void>;
    showButlerHomeWindow: boolean;
}

export const DisplayConfigForm: React.FC<DisplayConfigFormProps> = ({
    form,
    onSave,
    showButlerHomeWindow,
}) => {
    const previousNotificationValue = useRef<boolean | undefined>(undefined);
    const { themes, themeInfo } = useSyntectThemes();
    const mcpToolComponents = useMcpToolComponentRegistrySnapshot();
    const [pluginThemeOptions, setPluginThemeOptions] = useState<Array<{ value: string; label: string }>>([]);

    useEffect(() => {
        ensureBuiltinMcpToolComponentsRegistered();
    }, []);

    useEffect(() => {
        let disposed = false;
        const loadPluginThemes = async () => {
            try {
                await pluginRuntime.loadPlugins();
                const registeredThemes = await pluginRuntime.listDisplayThemes();
                if (disposed) {
                    return;
                }
                setPluginThemeOptions(
                    registeredThemes.map((theme) => ({
                        value: theme.id,
                        label: theme.label,
                    }))
                );
            } catch (error) {
                console.error("[DisplayConfigForm] Failed to load plugin themes:", error);
                if (!disposed) {
                    setPluginThemeOptions([]);
                }
            }
        };

        loadPluginThemes();
        const unlistenRegistryChanged = listen("plugin_registry_changed", () => {
            loadPluginThemes();
        });
        return () => {
            disposed = true;
            unlistenRegistryChanged.then((unlisten) => unlisten());
        };
    }, []);

    const themeOptions = useMemo(() => {
        const optionMap = new Map<string, { value: string; label: string }>([
            ["default", { value: "default", label: "默认主题" }],
            ["newyear", { value: "newyear", label: "新年主题" }],
        ]);
        pluginThemeOptions.forEach((option) => {
            optionMap.set(option.value, option);
        });
        return [...optionMap.values()];
    }, [pluginThemeOptions]);

    const colorModeOptions = [
        { value: "light", label: "浅色" },
        { value: "dark", label: "深色" },
        { value: "system", label: "跟随系统" },
    ];

    const markdownRenderOptions = [
        { value: "enabled", label: "开启" },
        { value: "disabled", label: "关闭" },
    ];

    const mcpToolComponentOptions = useMemo(() => {
        const optionMap = new Map<string, { value: string; label: string }>([
            [AUTO_MCP_TOOL_COMPONENT_ID, { value: AUTO_MCP_TOOL_COMPONENT_ID, label: "自动匹配组件" }],
            [DEFAULT_MCP_TOOL_COMPONENT_ID, { value: DEFAULT_MCP_TOOL_COMPONENT_ID, label: "默认工具调用组件" }],
        ]);
        mcpToolComponents.forEach((component) => {
            optionMap.set(component.id, {
                value: component.id,
                label: component.ownerCode === "builtin"
                    ? component.label
                    : `${component.label} (${component.ownerCode})`,
            });
        });
        return [...optionMap.values()];
    }, [mcpToolComponents]);

    const defaultHomeWindowOptions = useMemo(() => {
        const options = [
            { value: "ask", label: "Ask 悬浮窗" },
            { value: "chat_ui", label: "Chat 主窗口" },
        ];
        if (showButlerHomeWindow) {
            options.push({ value: "butler_experiment", label: "总管家实验窗口" });
        }
        return options;
    }, [showButlerHomeWindow]);

    useEffect(() => {
        if (!showButlerHomeWindow && form.getValues("default_home_window") === "butler_experiment") {
            form.setValue("default_home_window", "ask");
        }
    }, [form, showButlerHomeWindow]);

    const syntectThemeOptions = useMemo(() => {
        if (!themes || themes.length === 0) return null;
        return [...themes]
            .sort((a, b) => a.localeCompare(b))
            .map((name) => ({ value: name, label: name }));
    }, [themes]);

    const syntectThemeOptionsByMode = useMemo(() => {
        if (!themeInfo || themeInfo.length === 0) return null;
        const sorted = [...themeInfo].sort((a, b) => a.name.localeCompare(b.name));
        return {
            light: sorted.filter((item) => !item.is_dark).map((item) => ({ value: item.name, label: item.name })),
            dark: sorted.filter((item) => item.is_dark).map((item) => ({ value: item.name, label: item.name })),
        };
    }, [themeInfo]);

    const fallbackLightOptions = useMemo(() => {
        return AVAILABLE_CODE_THEMES.filter((theme) => theme.category === "light").map((theme) => ({
            value: theme.id,
            label: theme.name,
        }));
    }, []);

    const fallbackDarkOptions = useMemo(() => {
        return AVAILABLE_CODE_THEMES.filter((theme) => theme.category === "dark").map((theme) => ({
            value: theme.id,
            label: theme.name,
        }));
    }, []);

    const lightCodeThemeOptions = syntectThemeOptionsByMode?.light?.length
        ? syntectThemeOptionsByMode.light
        : syntectThemeOptions ?? fallbackLightOptions;
    const darkCodeThemeOptions = syntectThemeOptionsByMode?.dark?.length
        ? syntectThemeOptionsByMode.dark
        : syntectThemeOptions ?? fallbackDarkOptions;

    const handleSaveDisplayConfig = useCallback(async () => {
        const values = form.getValues();
        const currentNotificationValue = values.notification_on_completion;

        // 检查通知设置是否从 false 变为 true
        const notificationJustEnabled = 
            previousNotificationValue.current === false && 
            currentNotificationValue === true;

        // 如果用户刚刚开启了通知，需要检查和申请权限
        if (notificationJustEnabled) {
            try {
                let permissionGranted = await isPermissionGranted();

                if (!permissionGranted) {
                    const permission = await requestPermission();
                    permissionGranted = permission === "granted";
                }

                if (!permissionGranted) {
                    toast.error("通知权限未获取，无法开启系统通知功能");
                    // 重置开关状态
                    form.setValue("notification_on_completion", false);
                    return;
                }

                // 权限获取成功，发送测试通知
                sendNotification({
                    title: "AIPP - 系统通知已开启",
                    body: "AI 消息完成时将发送系统通知",
                });
                toast.success("通知权限获取成功，已发送测试通知");
            } catch (e) {
                toast.error("获取通知权限时发生错误: " + e);
                form.setValue("notification_on_completion", false);
                return;
            }
        }

        try {
            await onSave();

            // 更新上次的通知设置值
            previousNotificationValue.current = currentNotificationValue;

            // 发出主题变化事件，通知其他窗口和组件
            await emit("theme-changed", {
                mode: values.color_mode,
                code_theme_light: values.code_theme_light,
                code_theme_dark: values.code_theme_dark,
            });

            // 发出显示配置变化事件，通知聊天界面实时更新
            await emit("display-config-changed", {
                merge_assistant_messages: values.merge_assistant_messages,
                show_thinking: values.show_thinking,
                preview_code_show_toolbar: values.preview_code_show_toolbar,
                mcp_tool_call_component_id: values.mcp_tool_call_component_id,
            });

            toast.success("显示配置保存成功");
        } catch (e) {
            toast.error("保存显示配置失败: " + e);
        }
    }, [form, onSave]);

    const DISPLAY_FORM_CONFIG = [
        {
            key: "default_home_window",
            config: {
                type: "select" as const,
                label: "默认主页窗口",
                options: defaultHomeWindowOptions,
                tooltip: "影响应用启动、托盘点击和唤醒时默认打开的主窗口",
            },
        },
        {
            key: "theme",
            config: {
                type: "select" as const,
                label: "系统外观主题",
                options: themeOptions,
            },
        },
        {
            key: "color_mode",
            config: {
                type: "select" as const,
                label: "深浅色模式",
                options: colorModeOptions,
            },
        },
        {
            key: "code_theme_light",
            config: {
                type: "select" as const,
                label: "浅色模式代码主题",
                options: lightCodeThemeOptions,
            },
        },
        {
            key: "code_theme_dark",
            config: {
                type: "select" as const,
                label: "深色模式代码主题",
                options: darkCodeThemeOptions,
            },
        },
        {
            key: "user_message_markdown_render",
            config: {
                type: "select" as const,
                label: "用户消息Markdown渲染",
                options: markdownRenderOptions,
            },
        },
        {
            key: "notification_on_completion",
            config: {
                type: "switch" as const,
                label: "消息完成时发送系统通知",
                tooltip: "AI消息生成完成时发送系统通知提醒",
            },
        },
        {
            key: "merge_assistant_messages",
            config: {
                type: "switch" as const,
                label: "合并助手消息",
                tooltip: "开启后，同一轮对话中的思考、工具调用和回复将合并在一个气泡中展示",
            },
        },
        {
            key: "show_thinking",
            config: {
                type: "switch" as const,
                label: "展示思考过程",
                tooltip: "关闭后，AI 思考过程仅显示为一个加载指示器，不展示思考内容",
            },
        },
        {
            key: "preview_code_show_toolbar",
            config: {
                type: "switch" as const,
                label: "preview_code 展示工具栏",
                tooltip: "开启后，preview_code 组件显示标题、工具名称、文件类型和隐藏按钮",
            },
        },
        {
            key: "mcp_tool_call_component_id",
            config: {
                type: "select" as const,
                label: "MCP工具调用组件",
                options: mcpToolComponentOptions,
                tooltip: "控制聊天中 MCP 工具调用卡片的视觉组件。自动匹配会优先使用为特定工具注册的组件，否则使用默认组件",
            },
        },
    ];

    return (
        <ConfigForm
            title="显示"
            description="配置默认主页窗口、系统外观主题、深浅色模式和用户消息渲染方式"
            config={DISPLAY_FORM_CONFIG}
            layout="default"
            classNames="bottom-space"
            useFormReturn={form}
            onSave={handleSaveDisplayConfig}
        />
    );
};
