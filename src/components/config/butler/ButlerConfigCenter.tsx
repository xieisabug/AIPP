import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Controller, useForm } from "react-hook-form";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
    AlertTriangle,
    Brain,
    CalendarHeart,
    CircleHelp,
    FolderLock,
    MessageSquareCode,
    Pencil,
    Plus,
    Sliders,
    Trash2,
    UserCog,
    Wand2,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Form, FormControl, FormItem, FormMessage } from "@/components/ui/form";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Textarea } from "@/components/ui/textarea";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ConfigPageLoading } from "@/components/common";
import { ConfigFooterLayout, ConfigSaveFooter } from "@/components/common/ConfigSaveFooter";
import { FolderPicker } from "@/components/config/FolderPicker";
import { SettingsGroup, SettingRowText, settingRowClassName, stackedRowClassName } from "@/components/config/SettingsGroup";
import SettingsLink from "@/components/config/SettingsLink";
import {
    buildExperimentalConfigFormValues,
    EXPERIMENTAL_CONFIG_DEFAULT_VALUES,
    saveExperimentalConfigValues,
    type ExperimentalConfigFormState,
} from "@/components/config/feature/forms/experimentalConfigShared";
import {
    buildButlerWorkspaceConfig,
    BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
    serializeButlerWorkspaceConfig,
    type TrustedWorkspace,
} from "@/components/butler/butlerWorkspaceConfig";
import { ButlerOnboardingWizard } from "@/components/butler/ButlerOnboardingWizard";
import ButlerAvatar from "@/components/butler/ButlerAvatar";
import ButlerProfileDialog from "@/components/butler/ButlerProfileDialog";
import ButlerProactiveSettings from "@/components/butler/ButlerProactiveSettings";
import ButlerProactiveCenter from "@/components/butler/ButlerProactiveCenter";
import ButlerMemoryPanel from "@/components/butler/ButlerMemoryPanel";
import { useButlerProactiveConfig } from "@/components/butler/useButlerProactiveConfig";
import { appearanceKey, defaultAppearance, parseAppearance, type ButlerAppearance } from "@/components/butler/butlerAppearance";
import { isDecisionRequestMode } from "@/components/config/llmModelTypes";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useModels } from "@/hooks/useModels";
import { useUnsavedLeaveGuard } from "@/hooks/useUnsavedLeaveGuard";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";
import { getErrorMessage } from "@/utils/error";

const HELPER_MODEL_NONE = "__none__";

interface FeishuRuntimeStatus {
    butler_enabled: boolean;
    enabled: boolean;
    configured: boolean;
    secret_configured: boolean;
    running: boolean;
    connected: boolean;
    app_id?: string | null;
    base_url?: string | null;
    allow_p2p: boolean;
    allow_group: boolean;
    group_require_mention: boolean;
    last_error?: string | null;
    last_event_at?: string | null;
    last_status_at?: string | null;
    status_detail?: string | null;
    status_text: string;
}

type ButlerTabId = "basic" | "workspaces" | "proactive" | "feishu" | "advanced" | "memory";

interface ButlerConfigCenterProps {
    /** Embedded inside ButlerExperimentWindow settings dialog (omits outer page title & leave guard). */
    embedded?: boolean;
    /** Register configLeaveGuard when running as a top-level settings page. */
    enableLeaveGuard?: boolean;
    subNav?: string;
    onSubNavConsumed?: () => void;
    navigateTo?: (menuKey: string, subNav?: string) => void;
    /** Optional callback when embedded inside Butler window to open its existing proactive center dialog. */
    onOpenProactiveCenter?: () => void;
    /** Optional callback after saving config or completing onboarding. */
    onSaved?: () => void;
}

const isEnabledValue = (value: unknown) => value === true || value === "true";

export const ButlerConfigCenter: React.FC<ButlerConfigCenterProps> = ({
    embedded = false,
    enableLeaveGuard = false,
    subNav,
    onSubNavConsumed,
    navigateTo,
    onOpenProactiveCenter,
    onSaved,
}) => {
    const { featureConfig, saveFeatureConfig, loadFeatureConfig, loading } = useFeatureConfig();
    const proactive = useButlerProactiveConfig();

    const [tab, setTab] = useState<ButlerTabId>("basic");
    const [anchor, setAnchor] = useState<string | null>(null);
    const [initialLoaded, setInitialLoaded] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [isOnboardingOpen, setIsOnboardingOpen] = useState(false);
    const [isAvatarDialogOpen, setIsAvatarDialogOpen] = useState(false);
    const [localProactiveCenterOpen, setLocalProactiveCenterOpen] = useState(false);
    const [feishuStatus, setFeishuStatus] = useState<FeishuRuntimeStatus | null>(null);
    const [newTrustedPath, setNewTrustedPath] = useState("");
    const [newTrustedDesc, setNewTrustedDesc] = useState("");

    const form = useForm<ExperimentalConfigFormState>({
        defaultValues: { ...EXPERIMENTAL_CONFIG_DEFAULT_VALUES },
    });
    const baselineFormRef = useRef<string>("");

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        const validTabs: ButlerTabId[] = ["basic", "workspaces", "proactive", "feishu", "advanced", "memory"];
        if (parsed.tab && validTabs.includes(parsed.tab as ButlerTabId)) {
            setTab(parsed.tab as ButlerTabId);
        } else if (parsed.anchor === "butler-workspaces" || parsed.anchor === "workspaces") {
            setTab("workspaces");
        } else if (parsed.anchor === "butler-proactive" || parsed.anchor === "proactive") {
            setTab("proactive");
        } else if (parsed.anchor === "butler-feishu" || parsed.anchor === "feishu") {
            setTab("feishu");
        } else if (parsed.anchor === "butler-controls") {
            setTab("advanced");
        } else if (parsed.anchor === "butler-memory") {
            setTab("memory");
        }
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null), `${tab}:${initialLoaded}`);

    useEffect(() => {
        if (loading) return;
        const values = buildExperimentalConfigFormValues(featureConfig);
        baselineFormRef.current = JSON.stringify(values);
        form.reset(values);
        setInitialLoaded(true);
    }, [loading, featureConfig, form]);

    const loadFeishuStatus = useCallback(async () => {
        try {
            const status = await invoke<FeishuRuntimeStatus>("get_butler_feishu_runtime_status");
            setFeishuStatus(status);
        } catch {
            // ignore when backend not ready
        }
    }, []);

    useEffect(() => {
        void loadFeishuStatus();
        const unlistenPromise = listen<FeishuRuntimeStatus>("butler_feishu_status_changed", (event) => {
            setFeishuStatus(event.payload);
        });
        return () => {
            void unlistenPromise.then((unlisten) => unlisten());
        };
    }, [loadFeishuStatus]);

    const watched = form.watch();
    const butlerEnabled = isEnabledValue(watched.butler_experiment_enabled);
    const feishuEnabled = isEnabledValue(watched.butler_feishu_enabled);
    const butlerModelId = String(watched.butler_model_id || "");
    const butlerDisplayName = String(watched.butler_display_name || "总管家").trim() || "总管家";
    const butlerMainWorkspacePath = String(watched.butler_main_workspace_path || "");
    const butlerMainWorkspaceDescription = String(
        watched.butler_main_workspace_description || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION
    );
    const feishuAppId = String(watched.butler_feishu_app_id || "");
    const feishuAppSecret = String(watched.butler_feishu_app_secret || "");
    const feishuBaseUrl = String(watched.butler_feishu_base_url || "https://open.feishu.cn");
    const trustAllWorkspaces = isEnabledValue(watched.butler_trust_all_workspaces);
    const trustedPathsRaw = String(watched.butler_trusted_workspaces || "");

    const { models, loading: modelsLoading, error: modelsError } = useModels(butlerEnabled);
    const { models: dispatchModels, loading: dispatchModelsLoading } = useModels(butlerEnabled, {
        includeDecision: true,
    });

    const modelOptions = useMemo(
        () => models.map((m) => ({ value: `${m.code}%%${m.llm_provider_id}`, label: m.name })),
        [models]
    );
    const dispatchModelOptions = useMemo(
        () =>
            dispatchModels.map((m) => ({
                value: `${m.code}%%${m.llm_provider_id}`,
                label: m.is_decision || isDecisionRequestMode(m.request_mode) ? `${m.name} [决策]` : m.name,
            })),
        [dispatchModels]
    );

    const defaultLightModelValue = featureConfig.get("conversation_summary")?.get("default_light_model") || "";
    const defaultDecisionModelValue = featureConfig.get("conversation_summary")?.get("default_decision_model") || "";
    const defaultLightLabel = modelOptions.find((o) => o.value === defaultLightModelValue)?.label;
    const defaultDecisionLabel = dispatchModelOptions.find((o) => o.value === defaultDecisionModelValue)?.label;

    const chiefAppearance: ButlerAppearance = useMemo(
        () => parseAppearance(featureConfig.get("butler_appearance")?.get(appearanceKey(undefined))) ?? defaultAppearance,
        [featureConfig]
    );

    const workspaceConfig = useMemo(
        () =>
            buildButlerWorkspaceConfig({
                mainWorkspacePath: butlerMainWorkspacePath,
                mainWorkspaceDescription: butlerMainWorkspaceDescription,
                trustedWorkspacesRaw: trustedPathsRaw,
            }),
        [butlerMainWorkspacePath, butlerMainWorkspaceDescription, trustedPathsRaw]
    );
    const trustedWorkspaces = workspaceConfig.trustedWorkspaces;

    const setTrustedWorkspaces = useCallback(
        (ws: TrustedWorkspace[]) => {
            const nextConfig = serializeButlerWorkspaceConfig({
                mainWorkspacePath: butlerMainWorkspacePath,
                mainWorkspaceDescription: butlerMainWorkspaceDescription,
                trustedWorkspaces: ws,
            });
            form.setValue("butler_main_workspace_path", nextConfig.mainWorkspacePath, { shouldDirty: true });
            form.setValue(
                "butler_main_workspace_description",
                nextConfig.mainWorkspaceDescription || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
                { shouldDirty: true }
            );
            form.setValue("butler_trusted_workspaces", nextConfig.trustedWorkspacesRaw, { shouldDirty: true });
        },
        [butlerMainWorkspaceDescription, butlerMainWorkspacePath, form]
    );

    const setMainWorkspace = useCallback(
        (path: string, description: string) => {
            const nextConfig = serializeButlerWorkspaceConfig({
                mainWorkspacePath: path,
                mainWorkspaceDescription: description,
                trustedWorkspaces,
            });
            form.setValue("butler_main_workspace_path", nextConfig.mainWorkspacePath, { shouldDirty: true });
            form.setValue(
                "butler_main_workspace_description",
                nextConfig.mainWorkspaceDescription || BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION,
                { shouldDirty: true }
            );
            form.setValue("butler_trusted_workspaces", nextConfig.trustedWorkspacesRaw, { shouldDirty: true });
        },
        [form, trustedWorkspaces]
    );

    const handleAddTrustedPath = () => {
        const trimmed = newTrustedPath.trim();
        if (!trimmed) return;
        if (trimmed === butlerMainWorkspacePath.trim()) {
            toast.error("该路径已被设置为主工作区");
            return;
        }
        if (trustedWorkspaces.some((w) => w.path === trimmed)) {
            toast.error("该路径已存在");
            return;
        }
        setTrustedWorkspaces([...trustedWorkspaces, { path: trimmed, description: newTrustedDesc.trim() }]);
        setNewTrustedPath("");
        setNewTrustedDesc("");
    };

    const formDirty = initialLoaded && JSON.stringify(watched) !== baselineFormRef.current;
    const proactiveDirty =
        Boolean(proactive.config && proactive.saved) &&
        (Boolean(proactive.config?.release_pending) ||
            JSON.stringify(proactive.config) !== JSON.stringify(proactive.saved));
    const isDirty = formDirty || proactiveDirty;

    const handleSave = useCallback(async () => {
        if (butlerEnabled && !butlerModelId) {
            toast.error("请先在「基础与模型」中选择总管家主模型");
            setTab("basic");
            throw new Error("missing butler_model_id");
        }
        if (butlerEnabled && !butlerMainWorkspacePath.trim()) {
            toast.error("请先在「工作区与权限」中配置主工作区");
            setTab("workspaces");
            throw new Error("missing butler_main_workspace_path");
        }
        if (butlerEnabled && feishuEnabled && !feishuAppId.trim()) {
            toast.error("请先在「飞书接入」中填写飞书 App ID");
            setTab("feishu");
            throw new Error("missing butler_feishu_app_id");
        }
        if (butlerEnabled && feishuEnabled && !feishuAppSecret.trim() && !feishuStatus?.secret_configured) {
            toast.error("请先在「飞书接入」中填写飞书 App Secret");
            setTab("feishu");
            throw new Error("missing butler_feishu_app_secret");
        }

        setIsSaving(true);
        try {
            const values = form.getValues();
            await saveExperimentalConfigValues(saveFeatureConfig, values, "butler");
            if (butlerEnabled && feishuEnabled && feishuAppSecret.trim()) {
                await invoke("save_butler_feishu_secret", {
                    appSecret: feishuAppSecret.trim(),
                    app_secret: feishuAppSecret.trim(),
                });
                form.setValue("butler_feishu_app_secret", "");
            }
            if (butlerEnabled) {
                await proactive.save();
            }
            const latestFeishuStatus = await invoke<FeishuRuntimeStatus>("refresh_butler_feishu_runtime_command");
            setFeishuStatus(latestFeishuStatus);
            const cleaned = { ...form.getValues(), butler_feishu_app_secret: "" };
            baselineFormRef.current = JSON.stringify(cleaned);
            toast.success("总管家配置已保存");
            onSaved?.();
        } catch (error) {
            toast.error("保存总管家配置失败：" + getErrorMessage(error));
            throw error;
        } finally {
            setIsSaving(false);
        }
    }, [
        butlerEnabled,
        butlerModelId,
        butlerMainWorkspacePath,
        feishuEnabled,
        feishuAppId,
        feishuAppSecret,
        feishuStatus?.secret_configured,
        form,
        saveFeatureConfig,
        proactive,
        onSaved,
    ]);

    const handleDiscard = useCallback(() => {
        if (baselineFormRef.current) {
            try {
                form.reset(JSON.parse(baselineFormRef.current));
            } catch {
                // ignore
            }
        }
        if (proactive.saved) {
            proactive.setConfig(proactive.saved);
        }
    }, [form, proactive]);

    const { dialog: leaveDialog } = useUnsavedLeaveGuard({
        isDirty,
        onSave: handleSave,
        onDiscard: handleDiscard,
        enabled: enableLeaveGuard,
    });

    const handleRefreshFeishu = async () => {
        setIsSaving(true);
        try {
            const status = await invoke<FeishuRuntimeStatus>("refresh_butler_feishu_runtime_command");
            setFeishuStatus(status);
            toast.success("飞书机器人已重载");
        } catch (error) {
            toast.error("重连飞书机器人失败：" + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    };

    const handleClearFeishuSecret = async () => {
        setIsSaving(true);
        try {
            await invoke("clear_butler_feishu_secret");
            form.setValue("butler_feishu_app_secret", "");
            const status = await invoke<FeishuRuntimeStatus>("refresh_butler_feishu_runtime_command");
            setFeishuStatus(status);
            toast.success("飞书 App Secret 已清除");
        } catch (error) {
            toast.error("清除飞书密钥失败：" + getErrorMessage(error));
        } finally {
            setIsSaving(false);
        }
    };

    const handleSaveChiefProfile = async (nextName: string, nextAppearance: ButlerAppearance) => {
        await saveFeatureConfig("experimental", { butler_display_name: nextName }, true);
        await saveFeatureConfig("butler_appearance", {
            [appearanceKey(undefined)]: JSON.stringify(nextAppearance),
        }, true);
        form.setValue("butler_display_name", nextName);
        if (baselineFormRef.current) {
            try {
                const parsed = JSON.parse(baselineFormRef.current);
                parsed.butler_display_name = nextName;
                baselineFormRef.current = JSON.stringify(parsed);
            } catch {
                // ignore
            }
        }
        toast.success("总管家名称与形象已更新");
    };

    const renderToggleRow = (name: keyof ExperimentalConfigFormState, label: React.ReactNode, description?: React.ReactNode) => (
        <Controller
            control={form.control}
            name={name}
            render={({ field }) => (
                <FormItem className={settingRowClassName}>
                    <SettingRowText label={label} description={description} />
                    <FormControl>
                        <Switch
                            checked={isEnabledValue(field.value)}
                            onCheckedChange={(checked) => field.onChange(checked ? "true" : "false")}
                        />
                    </FormControl>
                </FormItem>
            )}
        />
    );

    if (!initialLoaded) {
        return <ConfigPageLoading />;
    }

    const footerActions = (
        <div className="flex items-center justify-between gap-3">
            <div className="flex items-center gap-2">
                <Button type="button" disabled={isSaving} onClick={() => void handleSave()}>
                    {isSaving ? "保存中…" : "保存配置"}
                </Button>
                {isDirty ? (
                    <Button type="button" variant="outline" disabled={isSaving} onClick={handleDiscard}>
                        放弃更改
                    </Button>
                ) : null}
            </div>
            {isDirty ? (
                <span className="text-xs text-muted-foreground">有未保存的更改</span>
            ) : null}
        </div>
    );

    const headerBar = (
        <div className="flex flex-wrap items-start justify-between gap-4">
            {!embedded ? (
                <div className="min-w-0">
                    <div className="flex items-center gap-2">
                        <h2 className="text-xl font-semibold tracking-tight">总管家</h2>
                        {isDirty ? (
                            <span className="rounded-full border border-foreground/30 bg-muted px-2 py-0.5 text-xs font-medium">
                                未保存
                            </span>
                        ) : null}
                    </div>
                    <p className="mt-1 text-sm text-muted-foreground">
                        管理桌面管家工作台、模型、可信工作区、主动整理与关怀、飞书接入及长期记忆。
                    </p>
                </div>
            ) : (
                <div className="flex items-center gap-2">
                    <span className="text-sm font-medium">总管家模式</span>
                    {isDirty ? (
                        <span className="rounded-full border border-foreground/30 bg-muted px-2 py-0.5 text-xs font-medium">
                            未保存
                        </span>
                    ) : null}
                </div>
            )}
            <div className="flex shrink-0 items-center gap-3">
                {butlerEnabled ? (
                    <Button
                        type="button"
                        variant="outline"
                        size="sm"
                        onClick={() => setIsOnboardingOpen(true)}
                    >
                        <Wand2 className="mr-1.5 h-3.5 w-3.5" />
                        配置向导
                    </Button>
                ) : null}
                <Controller
                    control={form.control}
                    name="butler_experiment_enabled"
                    render={({ field }) => (
                        <label className="flex cursor-pointer items-center gap-2 text-sm font-medium">
                            <Switch
                                aria-label="启用总管家模式"
                                checked={isEnabledValue(field.value)}
                                onCheckedChange={(checked) => field.onChange(checked ? "true" : "false")}
                            />
                            <span>{isEnabledValue(field.value) ? "已启用" : "未启用"}</span>
                        </label>
                    )}
                />
            </div>
        </div>
    );

    const tabsNav = (
        <TabsList className="flex h-auto w-full flex-wrap justify-start gap-1 bg-muted/60 p-1">
            <TabsTrigger value="basic" className="gap-1.5 text-xs sm:text-sm">
                <UserCog className="h-3.5 w-3.5" />
                基础与模型
            </TabsTrigger>
            <TabsTrigger value="workspaces" className="gap-1.5 text-xs sm:text-sm">
                <FolderLock className="h-3.5 w-3.5" />
                工作区与权限
            </TabsTrigger>
            <TabsTrigger value="proactive" className="gap-1.5 text-xs sm:text-sm">
                <CalendarHeart className="h-3.5 w-3.5" />
                主动整理
            </TabsTrigger>
            <TabsTrigger value="feishu" className="gap-1.5 text-xs sm:text-sm">
                <MessageSquareCode className="h-3.5 w-3.5" />
                飞书接入
            </TabsTrigger>
            <TabsTrigger value="advanced" className="gap-1.5 text-xs sm:text-sm">
                <Sliders className="h-3.5 w-3.5" />
                高级控制
            </TabsTrigger>
            <TabsTrigger value="memory" className="gap-1.5 text-xs sm:text-sm">
                <Brain className="h-3.5 w-3.5" />
                记忆
            </TabsTrigger>
        </TabsList>
    );

    const bodyContent = (
        <Form {...form}>
            <div className={embedded ? "flex h-full min-h-0 flex-col" : "config-page-top w-full space-y-6 px-4 py-6 sm:px-6"}>
                {embedded ? (
                    <div className="shrink-0 space-y-3 border-b border-border/60 px-5 pt-4 pb-3">
                        {headerBar}
                        {butlerEnabled ? (
                            <Tabs value={tab} onValueChange={(next) => setTab(next as ButlerTabId)}>
                                {tabsNav}
                            </Tabs>
                        ) : null}
                    </div>
                ) : (
                    headerBar
                )}

                {!butlerEnabled ? (
                    <div className={embedded ? "flex-1 overflow-y-auto px-5 py-6" : ""}>
                        <div className="rounded-xl border border-dashed border-border bg-muted/20 p-6 text-center space-y-3">
                            <p className="text-sm font-medium">总管家模式尚未启用</p>
                            <p className="mx-auto max-w-md text-xs text-muted-foreground">
                                开启后将启用独立的总管家工作台、联系人长期私聊、多助手群聊协作、主动整理与关怀以及飞书接入能力。
                            </p>
                            <Button
                                type="button"
                                size="sm"
                                onClick={() => form.setValue("butler_experiment_enabled", "true", { shouldDirty: true })}
                            >
                                启用总管家模式
                            </Button>
                        </div>
                    </div>
                ) : (
                    <Tabs
                        value={tab}
                        onValueChange={(next) => setTab(next as ButlerTabId)}
                        className={embedded ? "min-h-0 flex-1 overflow-y-auto px-5 py-4" : "space-y-5"}
                    >
                        {!embedded ? tabsNav : null}

                        {/* Tab 1: 基础与模型 */}
                        <TabsContent value="basic" className="mt-0 space-y-5">
                            <SettingsGroup id="butler-basic" title="身份与形象" description="自定义总管家在工作台头部与联系人列表中的展示名称与形象。">
                                <div className={settingRowClassName}>
                                    <div className="flex items-center gap-3.5 min-w-0">
                                        <div className="flex h-14 w-14 shrink-0 items-center justify-center rounded-xl bg-muted/50">
                                            <ButlerAvatar id={0} chief size={48} appearance={chiefAppearance} />
                                        </div>
                                        <div className="min-w-0">
                                            <div className="truncate text-sm font-semibold">{butlerDisplayName}</div>
                                            <p className="text-xs text-muted-foreground">点击右侧按钮可自定义名称、形状、配色与配饰。</p>
                                        </div>
                                    </div>
                                    <Button
                                        type="button"
                                        variant="outline"
                                        size="sm"
                                        onClick={() => setIsAvatarDialogOpen(true)}
                                    >
                                        <Pencil className="mr-1.5 h-3.5 w-3.5" />
                                        编辑名称与形象
                                    </Button>
                                </div>
                            </SettingsGroup>

                            <SettingsGroup
                                title="核心模型配置"
                                description="未单独指定的辅助/决策模型将自动沿用「AI 与自动化」中的默认轻量模型与默认决策模型。"
                                action={
                                    <SettingsLink
                                        menu="ai-automation-config"
                                        subNav="#default-models"
                                        navigateTo={navigateTo}
                                        openWindow={embedded}
                                        variant="link"
                                    >
                                        配置默认模型
                                    </SettingsLink>
                                }
                            >
                                <Controller
                                    control={form.control}
                                    name="butler_model_id"
                                    render={({ field }) => (
                                        <FormItem className={settingRowClassName}>
                                            <SettingRowText
                                                label="总管家主模型"
                                                description="必填。主会话统筹协调、派发任务与汇总结果使用的核心模型。"
                                            />
                                            <div className="w-64 shrink-0 space-y-1">
                                                <FormControl>
                                                    <Select
                                                        value={String(field.value || "")}
                                                        onValueChange={field.onChange}
                                                        disabled={modelsLoading}
                                                    >
                                                        <SelectTrigger className="w-full" aria-label="总管家主模型">
                                                            <SelectValue
                                                                placeholder={modelsLoading ? "加载中…" : modelsError ? "加载失败" : "选择总管家主模型"}
                                                            />
                                                        </SelectTrigger>
                                                        <SelectContent>
                                                            {modelOptions.map((o) => (
                                                                <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                                                            ))}
                                                        </SelectContent>
                                                    </Select>
                                                </FormControl>
                                                <FormMessage />
                                            </div>
                                        </FormItem>
                                    )}
                                />

                                <Controller
                                    control={form.control}
                                    name="butler_helper_model_id"
                                    render={({ field }) => (
                                        <FormItem className={settingRowClassName}>
                                            <SettingRowText
                                                label={
                                                    <span className="inline-flex items-center gap-1">
                                                        小型辅助模型
                                                        <Tooltip>
                                                            <TooltipTrigger className="aipp-icon-button cursor-help" aria-label="小型辅助模型说明">
                                                                <CircleHelp className="size-3.5" />
                                                            </TooltipTrigger>
                                                            <TooltipContent className="max-w-xs text-xs">
                                                                用于实时进度状态总结、主动整理初筛等轻量后台任务；留空自动沿用默认轻量模型。
                                                            </TooltipContent>
                                                        </Tooltip>
                                                    </span>
                                                }
                                                description="留空时自动沿用默认轻量模型。"
                                            />
                                            <div className="w-64 shrink-0">
                                                <Select
                                                    value={String(field.value || HELPER_MODEL_NONE)}
                                                    onValueChange={(v) => field.onChange(v === HELPER_MODEL_NONE ? "" : v)}
                                                    disabled={modelsLoading}
                                                >
                                                    <SelectTrigger className="w-full" aria-label="小型辅助模型">
                                                        <SelectValue />
                                                    </SelectTrigger>
                                                    <SelectContent>
                                                        <SelectItem value={HELPER_MODEL_NONE}>
                                                            {defaultLightLabel ? `沿用默认轻量模型（${defaultLightLabel}）` : "沿用默认轻量模型 / 不使用"}
                                                        </SelectItem>
                                                        {modelOptions.map((o) => (
                                                            <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                                                        ))}
                                                    </SelectContent>
                                                </Select>
                                            </div>
                                        </FormItem>
                                    )}
                                />

                                <Controller
                                    control={form.control}
                                    name="butler_group_dispatch_model_id"
                                    render={({ field }) => (
                                        <FormItem className={settingRowClassName}>
                                            <SettingRowText
                                                label="群聊路由决策模型"
                                                description="用于管家群聊中自动判断由哪位成员回复以及接力循环审核（工具安全审核请前往「AI 与自动化」）。"
                                            />
                                            <div className="w-64 shrink-0">
                                                <Select
                                                    value={String(field.value || HELPER_MODEL_NONE)}
                                                    onValueChange={(v) => field.onChange(v === HELPER_MODEL_NONE ? "" : v)}
                                                    disabled={dispatchModelsLoading}
                                                >
                                                    <SelectTrigger className="w-full" aria-label="群聊路由决策模型">
                                                        <SelectValue />
                                                    </SelectTrigger>
                                                    <SelectContent>
                                                        <SelectItem value={HELPER_MODEL_NONE}>
                                                            {defaultDecisionLabel
                                                                ? `沿用默认决策模型（${defaultDecisionLabel}）`
                                                                : "沿用默认决策模型 / 辅助模型 / 总管家"}
                                                        </SelectItem>
                                                        {dispatchModelOptions.map((o) => (
                                                            <SelectItem key={o.value} value={o.value}>{o.label}</SelectItem>
                                                        ))}
                                                    </SelectContent>
                                                </Select>
                                            </div>
                                        </FormItem>
                                    )}
                                />
                            </SettingsGroup>
                        </TabsContent>

                        {/* Tab 2: 工作区与权限 */}
                        <TabsContent value="workspaces" className="mt-0 space-y-5">
                            <SettingsGroup
                                id="butler-workspaces"
                                title="可信工作区"
                                description="这些路径下的文件操作自动放行，描述会注入总管家提示词以帮助理解目录用途。"
                            >
                                <div className={stackedRowClassName}>
                                    <SettingRowText
                                        label="主工作区"
                                        description="必填。总管家会优先在这里组织任务、文件与产物。"
                                    />
                                    <FolderPicker
                                        value={butlerMainWorkspacePath}
                                        onChange={(value) => setMainWorkspace(value, butlerMainWorkspaceDescription)}
                                        placeholder="选择或输入主工作区目录路径"
                                    />
                                    <Input
                                        value={butlerMainWorkspaceDescription}
                                        onChange={(e) => setMainWorkspace(butlerMainWorkspacePath, e.target.value)}
                                        placeholder={BUTLER_MAIN_WORKSPACE_DEFAULT_DESCRIPTION}
                                    />
                                    {!butlerMainWorkspacePath.trim() ? (
                                        <p className="text-xs text-destructive">请先配置主工作区</p>
                                    ) : null}
                                </div>

                                {renderToggleRow(
                                    "butler_trust_all_workspaces",
                                    <span className="flex items-center gap-2">
                                        信任任何工作区
                                        <span className="inline-flex items-center gap-1 rounded border border-destructive/40 px-1.5 py-0.5 text-[11px] font-normal text-destructive">
                                            <AlertTriangle className="h-3 w-3" />
                                            危险
                                        </span>
                                    </span>,
                                    "开启后所有路径的文件操作均自动放行，不再弹出权限确认。仅建议在完全受信环境使用。"
                                )}

                                {!trustAllWorkspaces ? (
                                    <div className={stackedRowClassName}>
                                        <SettingRowText
                                            label="额外可信工作区"
                                            description="主工作区之外也允许自动放行的目录。"
                                        />
                                        <div className="flex items-center gap-2">
                                            <div className="min-w-0 flex-1">
                                                <FolderPicker
                                                    value={newTrustedPath}
                                                    onChange={setNewTrustedPath}
                                                    placeholder="选择或输入额外可信目录路径"
                                                />
                                            </div>
                                            <Button
                                                type="button"
                                                size="sm"
                                                variant="outline"
                                                onClick={handleAddTrustedPath}
                                                disabled={!newTrustedPath.trim()}
                                            >
                                                <Plus className="mr-1 h-4 w-4" />
                                                添加
                                            </Button>
                                        </div>
                                        <Input
                                            value={newTrustedDesc}
                                            onChange={(e) => setNewTrustedDesc(e.target.value)}
                                            placeholder="工作区描述（可选），例如：前端项目、Rust 后端代码仓库"
                                        />
                                        {trustedWorkspaces.length === 0 ? (
                                            <p className="text-xs text-muted-foreground">暂未配置额外可信工作区</p>
                                        ) : (
                                            <div className="max-h-64 divide-y overflow-y-auto rounded-md border">
                                                {trustedWorkspaces.map((ws) => (
                                                    <div key={ws.path} className="flex items-center gap-2 px-3 py-2">
                                                        <span className="min-w-0 flex-1 truncate font-mono text-xs" title={ws.path}>
                                                            {ws.path}
                                                        </span>
                                                        <Input
                                                            value={ws.description}
                                                            onChange={(e) =>
                                                                setTrustedWorkspaces(
                                                                    trustedWorkspaces.map((item) =>
                                                                        item.path === ws.path ? { ...item, description: e.target.value } : item
                                                                    )
                                                                )
                                                            }
                                                            placeholder="添加描述…"
                                                            className="h-8 w-52 shrink-0 text-xs"
                                                        />
                                                        <Button
                                                            type="button"
                                                            variant="ghost"
                                                            size="icon"
                                                            aria-label={`移除 ${ws.path}`}
                                                            onClick={() =>
                                                                setTrustedWorkspaces(trustedWorkspaces.filter((item) => item.path !== ws.path))
                                                            }
                                                            className="h-8 w-8 shrink-0 text-muted-foreground hover:text-destructive"
                                                        >
                                                            <Trash2 className="h-4 w-4" />
                                                        </Button>
                                                    </div>
                                                ))}
                                            </div>
                                        )}
                                    </div>
                                ) : null}
                            </SettingsGroup>
                        </TabsContent>

                        {/* Tab 3: 主动整理 */}
                        <TabsContent value="proactive" className="mt-0 space-y-5">
                            <div id="butler-proactive" className="space-y-5">
                                <ButlerProactiveSettings
                                    value={proactive.config}
                                    error={proactive.error}
                                    onChange={proactive.setConfig}
                                    onOpenCenter={() => {
                                        if (onOpenProactiveCenter) onOpenProactiveCenter();
                                        else setLocalProactiveCenterOpen(true);
                                    }}
                                />
                            </div>
                        </TabsContent>

                        {/* Tab 4: 飞书接入 */}
                        <TabsContent value="feishu" className="mt-0 space-y-5">
                            <SettingsGroup id="butler-feishu" title="飞书机器人接入" description="通过飞书长连接接收消息，并将总管家答复与高优先级主动提醒回发到飞书。">
                                {renderToggleRow(
                                    "butler_feishu_enabled",
                                    "启用飞书机器人接入",
                                    "开启后自动建立飞书开放平台长连接。"
                                )}

                                {feishuEnabled ? (
                                    <>
                                        <div className={stackedRowClassName}>
                                            <div className="flex flex-wrap items-start justify-between gap-3">
                                                <SettingRowText
                                                    label={
                                                        <span className="inline-flex items-center gap-2">
                                                            连接状态
                                                            <span
                                                                className={`inline-block h-2 w-2 rounded-full ${
                                                                    feishuStatus?.connected ? "bg-emerald-500" : "bg-amber-500"
                                                                }`}
                                                            />
                                                        </span>
                                                    }
                                                    description={feishuStatus?.status_text || "正在读取状态…"}
                                                />
                                                <div className="flex shrink-0 gap-2">
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        disabled={isSaving}
                                                        onClick={() => void handleRefreshFeishu()}
                                                    >
                                                        重连机器人
                                                    </Button>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        size="sm"
                                                        disabled={isSaving || !feishuStatus?.secret_configured}
                                                        onClick={() => void handleClearFeishuSecret()}
                                                    >
                                                        清除密钥
                                                    </Button>
                                                </div>
                                            </div>
                                            <div className="grid gap-x-4 gap-y-1 text-xs text-muted-foreground md:grid-cols-2">
                                                <div>App ID：{feishuStatus?.app_id || feishuAppId || "未配置"}</div>
                                                <div>Secret：{feishuStatus?.secret_configured ? "已加密保存" : "未保存"}</div>
                                                <div>最近事件：{feishuStatus?.last_event_at || "暂无"}</div>
                                                <div>更新时间：{feishuStatus?.last_status_at || "暂无"}</div>
                                                {feishuStatus?.last_error ? (
                                                    <div className="md:col-span-2 text-destructive">
                                                        最近错误：{feishuStatus.last_error}
                                                    </div>
                                                ) : null}
                                            </div>
                                        </div>

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_app_id"
                                            render={({ field }) => (
                                                <FormItem className={settingRowClassName}>
                                                    <SettingRowText label="飞书 App ID" description="飞书自建应用机器人的 App ID。" />
                                                    <div className="w-64 shrink-0">
                                                        <Input {...field} value={String(field.value || "")} placeholder="cli_xxx" />
                                                    </div>
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_app_secret"
                                            render={({ field }) => (
                                                <FormItem className={settingRowClassName}>
                                                    <SettingRowText
                                                        label="飞书 App Secret"
                                                        description="保存时单独加密存入系统钥匙串，不进入普通配置。"
                                                    />
                                                    <div className="w-64 shrink-0">
                                                        <Input
                                                            {...field}
                                                            type="password"
                                                            value={String(field.value || "")}
                                                            placeholder={feishuStatus?.secret_configured ? "已保存，留空保持不变" : "输入新的 App Secret"}
                                                        />
                                                    </div>
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_base_url"
                                            render={({ field }) => (
                                                <FormItem className={settingRowClassName}>
                                                    <SettingRowText label="开放平台域名" />
                                                    <div className="w-64 shrink-0">
                                                        <Select value={String(field.value || feishuBaseUrl)} onValueChange={field.onChange}>
                                                            <SelectTrigger className="w-full"><SelectValue /></SelectTrigger>
                                                            <SelectContent>
                                                                <SelectItem value="https://open.feishu.cn">飞书（中国大陆）</SelectItem>
                                                                <SelectItem value="https://open.larksuite.com">Lark（国际版）</SelectItem>
                                                            </SelectContent>
                                                        </Select>
                                                    </div>
                                                </FormItem>
                                            )}
                                        />

                                        {renderToggleRow("butler_feishu_receive_p2p", "接收单聊", "允许飞书用户私聊机器人驱动总管家。")}
                                        {renderToggleRow("butler_feishu_receive_group", "接收群聊", "允许群消息进入总管家。")}
                                        {renderToggleRow("butler_feishu_group_require_mention", "@ 或回复后才处理群消息", "群聊默认只处理带 mention 或回复机器人消息的文本。")}
                                        {renderToggleRow("butler_feishu_only_reply_feishu_originated", "仅回发飞书发起的对话", "开启后仅飞书触发的回合回发到飞书；关闭则桌面主会话尽量同步。")}

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_allowed_open_ids"
                                            render={({ field }) => (
                                                <FormItem className={stackedRowClassName}>
                                                    <SettingRowText label="允许对话的用户 Open ID 列表" description="留空不限制；支持换行、逗号或分号分隔。" />
                                                    <Textarea {...field} value={String(field.value || "")} placeholder={"ou_xxx\nou_yyy"} rows={3} />
                                                </FormItem>
                                            )}
                                        />

                                        <Controller
                                            control={form.control}
                                            name="butler_feishu_allowed_chat_ids"
                                            render={({ field }) => (
                                                <FormItem className={stackedRowClassName}>
                                                    <SettingRowText label="允许对话的群 Chat ID 列表" description="仅对群聊生效；留空不限制。" />
                                                    <Textarea {...field} value={String(field.value || "")} placeholder={"oc_xxx\noc_yyy"} rows={3} />
                                                </FormItem>
                                            )}
                                        />
                                    </>
                                ) : null}
                            </SettingsGroup>
                        </TabsContent>

                        {/* Tab 5: 高级控制 */}
                        <TabsContent value="advanced" className="mt-0 space-y-5">
                            <SettingsGroup
                                id="butler-controls"
                                title="管家能力熔断开关"
                                description="发生异常时可单独暂停某类新执行，保留已有记录与停止操作；重新开启后未过期排队工作可继续。"
                            >
                                {renderToggleRow("butler_task_dispatch_enabled", "新建独立任务", "允许总管家派发需要独立上下文的后台任务。")}
                                {renderToggleRow("butler_peer_collaboration_enabled", "发起成员协作", "关闭后不启动新的助手间协作请求；已有答复与回执仍正常返回。")}
                                {renderToggleRow("butler_work_continuation_enabled", "继续执行已有工作", "关闭后保留排队的续接要求，不再启动后续执行。")}
                            </SettingsGroup>
                        </TabsContent>

                        {/* Tab 6: 记忆 */}
                        <TabsContent value="memory" className="mt-0 space-y-4">
                            <div id="butler-memory">
                                <ButlerMemoryPanel assistantId={null} scope="chief" />
                            </div>
                        </TabsContent>
                    </Tabs>
                )}

                {tab !== "memory" ? (
                    embedded ? (
                        <div className="shrink-0 border-t border-border bg-card px-5 py-3">
                            {footerActions}
                        </div>
                    ) : (
                        <ConfigSaveFooter>{footerActions}</ConfigSaveFooter>
                    )
                ) : null}
            </div>

            <ButlerOnboardingWizard
                open={isOnboardingOpen}
                onOpenChange={setIsOnboardingOpen}
                existingModelId={butlerModelId}
                existingDisplayName={butlerDisplayName}
                existingTrustAll={trustAllWorkspaces}
                existingMainWorkspace={workspaceConfig.mainWorkspace}
                existingTrustedWorkspaces={trustedWorkspaces}
                existingFeishuEnabled={feishuEnabled}
                existingFeishuAppId={feishuAppId}
                existingFeishuBaseUrl={feishuBaseUrl}
                saveFeatureConfig={saveFeatureConfig}
                onComplete={() => {
                    void loadFeatureConfig();
                    onSaved?.();
                }}
            />

            {isAvatarDialogOpen ? (
                <ButlerProfileDialog
                    name={butlerDisplayName}
                    appearance={chiefAppearance}
                    onClose={() => setIsAvatarDialogOpen(false)}
                    onSave={handleSaveChiefProfile}
                />
            ) : null}

            {!onOpenProactiveCenter ? (
                <ButlerProactiveCenter
                    open={localProactiveCenterOpen}
                    onOpenChange={setLocalProactiveCenterOpen}
                    enabled={proactive.saved?.enabled === true}
                />
            ) : null}
        </Form>
    );

    if (embedded) {
        return (
            <>
                {bodyContent}
                {leaveDialog}
            </>
        );
    }

    return (
        <>
            <div className="flex h-full min-h-0 flex-col bg-background">
                <ConfigFooterLayout>
                    <div className="min-h-0 flex-1 overflow-y-auto">{bodyContent}</div>
                </ConfigFooterLayout>
            </div>
            {leaveDialog}
        </>
    );
};

export default ButlerConfigCenter;
