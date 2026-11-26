import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";
import { Check } from "lucide-react";
import { toast } from "sonner";
import { Switch } from "@/components/ui/switch";
import { Button } from "@/components/ui/button";
import { openOnboarding, useOnboardingEntry } from "@/components/onboarding/OnboardingSetup";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { SettingsGroup, SettingRowText, settingRowClassName } from "@/components/config/SettingsGroup";
import { ConfigPageLoading } from "@/components/common";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { useFeatureAvailableOnPlatform } from "@/lib/mobileUnsupported";
import { useSyntectThemes } from "@/hooks/highlight/useSyntectThemes";
import { pluginRuntime } from "@/services/PluginRuntime";
import {
    AUTO_MCP_TOOL_COMPONENT_ID,
    DEFAULT_MCP_TOOL_COMPONENT_ID,
    useMcpToolComponentRegistrySnapshot,
} from "@/services/mcpToolComponentRegistry";
import { AVAILABLE_CODE_THEMES } from "@/hooks/useCodeTheme";
import {
    buildDisplayFormValues,
    serializeDisplayFormValues,
    type DisplayFormValues,
} from "@/components/config/displayConfigFormValues";
import { withCurrentCodeThemeOption } from "@/components/config/feature/forms/DisplayConfigForm";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";
import SettingsPageShell from "./SettingsPageShell";

interface GeneralAppearanceConfigProps {
    subNav?: string;
    onSubNavConsumed?: () => void;
}

export const GeneralAppearanceConfig: React.FC<GeneralAppearanceConfigProps> = ({
    subNav,
    onSubNavConsumed,
}) => {
    const isMobilePlatform = useIsMobilePlatform();
    const canOpenOnboarding = useOnboardingEntry(!isMobilePlatform);
    const autostartAvailable = useFeatureAvailableOnPlatform("autostart");
    const { featureConfig, saveFeatureConfig, loading } = useFeatureConfig();
    const { themes, themeInfo } = useSyntectThemes();
    const mcpToolComponents = useMcpToolComponentRegistrySnapshot();

    const [initialLoaded, setInitialLoaded] = useState(false);
    const [values, setValues] = useState<DisplayFormValues>(() => buildDisplayFormValues());
    const [autostartEnabled, setAutostartEnabled] = useState<boolean>(false);
    const [autostartReady, setAutostartReady] = useState<boolean>(!autostartAvailable);
    const [autostartBusy, setAutostartBusy] = useState(false);
    const [pluginThemeOptions, setPluginThemeOptions] = useState<Array<{ value: string; label: string }>>([]);
    const [savedIndicator, setSavedIndicator] = useState(false);
    const savedTimerRef = useRef<number | null>(null);
    const [anchor, setAnchor] = useState<string | null>(null);

    const showSavedPulse = useCallback(() => {
        setSavedIndicator(true);
        if (savedTimerRef.current) window.clearTimeout(savedTimerRef.current);
        savedTimerRef.current = window.setTimeout(() => setSavedIndicator(false), 1600);
    }, []);

    useEffect(() => () => {
        if (savedTimerRef.current) window.clearTimeout(savedTimerRef.current);
    }, []);

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null), String(initialLoaded));

    // Sync form values from featureConfig
    const butlerEnabled = featureConfig.get("experimental")?.get("butler_experiment_enabled") === "true";
    useEffect(() => {
        if (loading) return;
        const next = buildDisplayFormValues(featureConfig.get("display"), featureConfig.get("experimental"));
        setValues(next);
        setInitialLoaded(true);
    }, [loading, featureConfig]);

    // Load autostart state on desktop
    useEffect(() => {
        if (!autostartAvailable) {
            setAutostartReady(true);
            return;
        }
        let active = true;
        invoke<boolean>("get_autostart_state")
            .then((enabled) => {
                if (active) setAutostartEnabled(Boolean(enabled));
            })
            .catch((e) => console.error("[GeneralAppearanceConfig] get_autostart_state failed:", e))
            .finally(() => {
                if (active) setAutostartReady(true);
            });
        return () => { active = false; };
    }, [autostartAvailable]);

    // Register builtin MCP tool components for selector options
    useEffect(() => {
        let disposed = false;
        void import("@/services/builtinMcpToolComponents").then((m) => {
            if (!disposed) m.ensureBuiltinMcpToolComponentsRegistered();
        });
        return () => { disposed = true; };
    }, []);

    // Load plugin themes
    useEffect(() => {
        let disposed = false;
        const loadPluginThemes = async () => {
            try {
                await pluginRuntime.loadPlugins();
                const registered = await pluginRuntime.listDisplayThemes();
                if (!disposed) {
                    setPluginThemeOptions(registered.map((t) => ({ value: t.id, label: t.label })));
                }
            } catch {
                if (!disposed) setPluginThemeOptions([]);
            }
        };
        void loadPluginThemes();
        const unlisten = listen("plugin_registry_changed", () => void loadPluginThemes());
        return () => {
            disposed = true;
            void unlisten.then((fn) => fn());
        };
    }, []);

    const themeOptions = useMemo(() => {
        const optionMap = new Map<string, { value: string; label: string }>([
            ["default", { value: "default", label: "默认主题" }],
            ["newyear", { value: "newyear", label: "新年主题" }],
        ]);
        pluginThemeOptions.forEach((opt) => optionMap.set(opt.value, opt));
        return [...optionMap.values()];
    }, [pluginThemeOptions]);

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
        if (butlerEnabled) {
            options.push({ value: "butler_experiment", label: "总管家工作台" });
        }
        return options;
    }, [butlerEnabled]);

    const syntectThemeOptions = useMemo(() => {
        if (!themes?.length) return null;
        return [...themes].sort((a, b) => a.localeCompare(b)).map((name) => ({ value: name, label: name }));
    }, [themes]);

    const syntectThemeOptionsByMode = useMemo(() => {
        if (!themeInfo?.length) return null;
        const sorted = [...themeInfo].sort((a, b) => a.name.localeCompare(b.name));
        return {
            light: sorted.filter((i) => !i.is_dark).map((i) => ({ value: i.name, label: i.name })),
            dark: sorted.filter((i) => i.is_dark).map((i) => ({ value: i.name, label: i.name })),
        };
    }, [themeInfo]);

    const fallbackLightOptions = useMemo(
        () => AVAILABLE_CODE_THEMES.filter((t) => t.category === "light").map((t) => ({ value: t.id, label: t.name })),
        []
    );
    const fallbackDarkOptions = useMemo(
        () => AVAILABLE_CODE_THEMES.filter((t) => t.category === "dark").map((t) => ({ value: t.id, label: t.name })),
        []
    );

    const lightCodeThemeOptions = useMemo(
        () => withCurrentCodeThemeOption(
            syntectThemeOptionsByMode?.light?.length ? syntectThemeOptionsByMode.light : syntectThemeOptions ?? fallbackLightOptions,
            values.code_theme_light
        ),
        [syntectThemeOptionsByMode, syntectThemeOptions, fallbackLightOptions, values.code_theme_light]
    );
    const darkCodeThemeOptions = useMemo(
        () => withCurrentCodeThemeOption(
            syntectThemeOptionsByMode?.dark?.length ? syntectThemeOptionsByMode.dark : syntectThemeOptions ?? fallbackDarkOptions,
            values.code_theme_dark
        ),
        [syntectThemeOptionsByMode, syntectThemeOptions, fallbackDarkOptions, values.code_theme_dark]
    );

    const persistDisplay = useCallback(
        async (nextValues: DisplayFormValues, opts?: { emitTheme?: boolean; emitDisplay?: boolean }) => {
            const prev = values;
            setValues(nextValues);
            try {
                await saveFeatureConfig("display", serializeDisplayFormValues(nextValues));
                if (opts?.emitTheme) {
                    await emit("theme-changed", {
                        mode: nextValues.color_mode,
                        code_theme_light: nextValues.code_theme_light,
                        code_theme_dark: nextValues.code_theme_dark,
                    });
                }
                if (opts?.emitDisplay) {
                    await emit("display-config-changed", {
                        merge_assistant_messages: nextValues.merge_assistant_messages,
                        show_thinking: nextValues.show_thinking,
                        preview_code_show_toolbar: nextValues.preview_code_show_toolbar,
                        mcp_tool_call_component_id: nextValues.mcp_tool_call_component_id,
                    });
                }
                showSavedPulse();
            } catch (e) {
                setValues(prev);
                toast.error("保存失败: " + e);
            }
        },
        [values, saveFeatureConfig, showSavedPulse]
    );

    const handleDefaultHomeWindowChange = useCallback(
        async (nextWindow: string) => {
            const prev = values;
            const resolved = nextWindow === "butler_experiment" && !butlerEnabled ? "ask" : nextWindow;
            setValues({ ...values, default_home_window: resolved });
            try {
                await saveFeatureConfig("experimental", { default_home_window: resolved }, true);
                showSavedPulse();
            } catch (e) {
                setValues(prev);
                toast.error("保存默认主页窗口失败: " + e);
            }
        },
        [values, butlerEnabled, saveFeatureConfig, showSavedPulse]
    );

    const handleAutostartChange = useCallback(
        async (checked: boolean) => {
            setAutostartBusy(true);
            try {
                await invoke("set_autostart", { enabled: checked });
                setAutostartEnabled(checked);
                showSavedPulse();
            } catch (e) {
                toast.error("设置开机自启动失败: " + e);
            } finally {
                setAutostartBusy(false);
            }
        },
        [showSavedPulse]
    );

    const handleNotificationToggle = useCallback(
        async (
            key: "notification_on_butler_reply" | "notification_on_completion" | "notification_on_permission",
            checked: boolean,
            testBody: string
        ) => {
            if (checked) {
                try {
                    let granted = await isPermissionGranted();
                    if (!granted) {
                        const permission = await requestPermission();
                        granted = permission === "granted";
                    }
                    if (!granted) {
                        toast.error("通知权限未获取，无法开启系统通知功能");
                        return;
                    }
                    sendNotification({ title: "AIPP - 系统通知已开启", body: testBody });
                } catch (e) {
                    toast.error("获取通知权限时发生错误: " + e);
                    return;
                }
            }
            await persistDisplay({ ...values, [key]: checked });
        },
        [values, persistDisplay]
    );

    if (!initialLoaded) {
        return <ConfigPageLoading />;
    }

    return (
        <SettingsPageShell
            title="常规与外观"
            description="配置启动行为、系统通知、界面主题与对话消息渲染样式，更改立即生效。"
            headerExtra={
                savedIndicator ? (
                    <span
                        role="status"
                        className="inline-flex items-center gap-1 rounded-full border border-border bg-muted/60 px-2.5 py-1 text-xs text-muted-foreground"
                    >
                        <Check className="h-3.5 w-3.5 text-emerald-500" />
                        已保存
                    </span>
                ) : null
            }
        >
            {!isMobilePlatform ? (
                <SettingsGroup id="startup" title="启动与窗口" description="控制应用启动方式与默认主界面。">
                    {canOpenOnboarding && (
                        <div className={settingRowClassName}>
                            <SettingRowText label="首次使用引导" description="跟随引导配置模型、助手和工作区。" />
                            <Button variant="outline" size="sm" onClick={openOnboarding}>打开引导</Button>
                        </div>
                    )}
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="默认主页窗口"
                            description="影响应用启动、托盘点击和唤醒时默认打开的主窗口。"
                        />
                        <div className="w-52 shrink-0">
                            <Select value={values.default_home_window} onValueChange={(v) => void handleDefaultHomeWindowChange(v)}>
                                <SelectTrigger className="w-full" aria-label="默认主页窗口">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    {defaultHomeWindowOptions.map((opt) => (
                                        <SelectItem key={opt.value} value={opt.value}>
                                            {opt.label}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    </div>
                    {autostartAvailable ? (
                        <div className={settingRowClassName}>
                            <SettingRowText
                                label="开机自启动"
                                description="应用将在系统登录启动时自动在后台运行。"
                            />
                            <Switch
                                aria-label="开机自启动"
                                checked={autostartEnabled}
                                disabled={autostartBusy || !autostartReady}
                                onCheckedChange={(checked) => void handleAutostartChange(checked)}
                            />
                        </div>
                    ) : null}
                </SettingsGroup>
            ) : null}

            <SettingsGroup id="notifications" title="系统通知" description="在后台等待或任务完成时发送系统级提醒，窗口聚焦时不打扰。">
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="管家收到回复时通知"
                        description="AIPP 在后台时，显示回复消息的总管家或助手名称。"
                    />
                    <Switch
                        aria-label="管家收到回复时通知"
                        checked={values.notification_on_butler_reply}
                        onCheckedChange={(checked) =>
                            void handleNotificationToggle(
                                "notification_on_butler_reply",
                                checked,
                                "管家模式收到回复时将发送系统通知"
                            )
                        }
                    />
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="消息或任务完成时通知"
                        description="AI 消息或 Agent 任务（Codex、Claude Code、ACP）完成时提醒。"
                    />
                    <Switch
                        aria-label="消息或任务完成时通知"
                        checked={values.notification_on_completion}
                        onCheckedChange={(checked) =>
                            void handleNotificationToggle(
                                "notification_on_completion",
                                checked,
                                "AI 消息或 Agent 任务完成时将发送系统通知"
                            )
                        }
                    />
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="等待审批时通知"
                        description="有权限请求或工具调用等待你手动确认时发送系统通知。"
                    />
                    <Switch
                        aria-label="等待审批时通知"
                        checked={values.notification_on_permission}
                        onCheckedChange={(checked) =>
                            void handleNotificationToggle(
                                "notification_on_permission",
                                checked,
                                "有权限或工具调用等待你审批时将发送系统通知"
                            )
                        }
                    />
                </div>
            </SettingsGroup>

            <SettingsGroup id="appearance" title="外观主题" description="设置界面风格、深浅色与代码块高亮配色。">
                <div className={settingRowClassName}>
                    <SettingRowText label="系统外观主题" description="支持插件扩展的自定义外观主题。" />
                    <div className="w-52 shrink-0">
                        <Select
                            value={values.theme}
                            onValueChange={(v) => void persistDisplay({ ...values, theme: v }, { emitTheme: true })}
                        >
                            <SelectTrigger className="w-full" aria-label="系统外观主题">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                {themeOptions.map((opt) => (
                                    <SelectItem key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText label="深浅色模式" />
                    <div className="w-52 shrink-0">
                        <Select
                            value={values.color_mode}
                            onValueChange={(v) => void persistDisplay({ ...values, color_mode: v }, { emitTheme: true })}
                        >
                            <SelectTrigger className="w-full" aria-label="深浅色模式">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                <SelectItem value="light">浅色</SelectItem>
                                <SelectItem value="dark">深色</SelectItem>
                                <SelectItem value="system">跟随系统</SelectItem>
                            </SelectContent>
                        </Select>
                    </div>
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText label="浅色模式代码主题" />
                    <div className="w-52 shrink-0">
                        <Select
                            value={values.code_theme_light}
                            onValueChange={(v) =>
                                void persistDisplay({ ...values, code_theme_light: v }, { emitTheme: true })
                            }
                        >
                            <SelectTrigger className="w-full" aria-label="浅色模式代码主题">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                {lightCodeThemeOptions.map((opt) => (
                                    <SelectItem key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText label="深色模式代码主题" />
                    <div className="w-52 shrink-0">
                        <Select
                            value={values.code_theme_dark}
                            onValueChange={(v) =>
                                void persistDisplay({ ...values, code_theme_dark: v }, { emitTheme: true })
                            }
                        >
                            <SelectTrigger className="w-full" aria-label="深色模式代码主题">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                {darkCodeThemeOptions.map((opt) => (
                                    <SelectItem key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
            </SettingsGroup>

            <SettingsGroup id="chat-display" title="对话显示" description="控制聊天气泡、思考过程与工具卡片的展示细节。">
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="合并助手消息"
                        description="开启后，同一轮对话中的思考、工具调用和回复将合并在一个气泡中展示。"
                    />
                    <Switch
                        aria-label="合并助手消息"
                        checked={values.merge_assistant_messages}
                        onCheckedChange={(checked) =>
                            void persistDisplay({ ...values, merge_assistant_messages: checked }, { emitDisplay: true })
                        }
                    />
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="展示思考过程"
                        description="关闭后，AI 思考过程仅显示为加载指示器，不展开思考正文。"
                    />
                    <Switch
                        aria-label="展示思考过程"
                        checked={values.show_thinking}
                        onCheckedChange={(checked) =>
                            void persistDisplay({ ...values, show_thinking: checked }, { emitDisplay: true })
                        }
                    />
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="用户消息 Markdown 渲染"
                        description="开启后，你自己发送的消息也会按 Markdown 语法格式化展示。"
                    />
                    <Switch
                        aria-label="用户消息 Markdown 渲染"
                        checked={values.user_message_markdown_render === "enabled"}
                        onCheckedChange={(checked) =>
                            void persistDisplay(
                                { ...values, user_message_markdown_render: checked ? "enabled" : "disabled" },
                                { emitDisplay: true }
                            )
                        }
                    />
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="preview_code 展示工具栏"
                        description="开启后，代码预览卡片显示标题、工具名称、文件类型和隐藏按钮。"
                    />
                    <Switch
                        aria-label="preview_code 展示工具栏"
                        checked={values.preview_code_show_toolbar}
                        onCheckedChange={(checked) =>
                            void persistDisplay({ ...values, preview_code_show_toolbar: checked }, { emitDisplay: true })
                        }
                    />
                </div>
                <div className={settingRowClassName}>
                    <SettingRowText
                        label="MCP 工具调用组件"
                        description="控制聊天中 MCP 工具调用卡片的视觉呈现方式。"
                    />
                    <div className="w-52 shrink-0">
                        <Select
                            value={values.mcp_tool_call_component_id}
                            onValueChange={(v) =>
                                void persistDisplay({ ...values, mcp_tool_call_component_id: v }, { emitDisplay: true })
                            }
                        >
                            <SelectTrigger className="w-full" aria-label="MCP 工具调用组件">
                                <SelectValue />
                            </SelectTrigger>
                            <SelectContent>
                                {mcpToolComponentOptions.map((opt) => (
                                    <SelectItem key={opt.value} value={opt.value}>
                                        {opt.label}
                                    </SelectItem>
                                ))}
                            </SelectContent>
                        </Select>
                    </div>
                </div>
            </SettingsGroup>
        </SettingsPageShell>
    );
};

export default GeneralAppearanceConfig;
