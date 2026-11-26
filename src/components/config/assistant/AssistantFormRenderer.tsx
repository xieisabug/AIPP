import React, { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
    Brain,
    Copy,
    Info,
    MoreHorizontal,
    Pencil,
    Share,
    Shield,
    Sliders,
    Trash2,
    UserCog,
    Wrench,
} from "lucide-react";
import { UseFormReturn } from "react-hook-form";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { ConfigFormField, type FieldOptionsData } from "@/components/ConfigForm";
import { ConfigSaveFooter } from "@/components/common/ConfigSaveFooter";
import { AssistantDetail } from "@/data/Assistant";
import { AssistantFormConfig, AssistantConfigApi } from "@/types/forms";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Form } from "@/components/ui/form";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import ButlerAvatar from "@/components/butler/ButlerAvatar";
import ButlerMemoryPanel from "@/components/butler/ButlerMemoryPanel";
import { appearanceKey, defaultAppearance, parseAppearance } from "@/components/butler/butlerAppearance";
import SettingsLink from "@/components/config/SettingsLink";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import { useFilteredProviders } from "@/hooks/useFilteredProviders";
import { useMcpServers } from "@/hooks/useMcpServers";

type AssistantTabId = "role" | "tools" | "permissions" | "advanced" | "memory";

const ROLE_KEYS = new Set([
    "assistantType",
    "model",
    "acp_provider",
    "agent_model",
    "reasoning_effort",
    "claude_effort",
    "acp_working_directory",
    "prompt",
]);

const TOOL_KEYS = new Set([
    "dynamic_mcp_loading_enabled",
    "mcp_config",
    "skills_config",
    "use_native_toolcall",
]);

const isPermissionKey = (key: string) =>
    key === "workspace_config" ||
    key === "tool_review_mode" ||
    key === "tool_review_model_hint" ||
    key.startsWith("codex_") ||
    key.startsWith("pi_") ||
    key.startsWith("kiro_") ||
    key.startsWith("antigravity_") ||
    key.startsWith("opencode_") ||
    key.startsWith("cursor_");

export function classifyAssistantFieldTab(item: AssistantFormConfig): Exclude<AssistantTabId, "memory"> {
    if (item.section === "extension") return "advanced";
    if (ROLE_KEYS.has(item.key)) return "role";
    if (TOOL_KEYS.has(item.key)) return "tools";
    if (isPermissionKey(item.key)) return "permissions";
    return "advanced";
}

interface AssistantFormRendererProps {
    currentAssistant: AssistantDetail | null;
    formConfig: AssistantFormConfig[];
    form: UseFormReturn<any>;
    /** Value-based dirty state from AssistantConfig (overrides RHF formState.isDirty). */
    isDirty: boolean;
    isSaving?: boolean;
    isLoading?: boolean;
    assistantConfigApi: AssistantConfigApi;
    onSave: () => void;
    onDiscard?: () => void;
    onCopy?: () => void;
    onDelete?: () => void;
    onEdit?: () => void;
    onShare?: () => void;
    navigateTo?: (menuKey: string, subNav?: string) => void;
}

export const AssistantFormRenderer: React.FC<AssistantFormRendererProps> = ({
    currentAssistant,
    formConfig,
    form,
    isDirty,
    isSaving,
    isLoading,
    assistantConfigApi,
    onSave,
    onDiscard,
    onCopy,
    onDelete,
    onEdit,
    onShare,
    navigateTo,
}) => {
    const mobile = useIsMobilePlatform();
    const { featureConfig } = useFeatureConfig();
    const [activeTab, setActiveTab] = useState<AssistantTabId>("role");
    const [compatibility, setCompatibility] = useState<{ id: number; reasons: string[] } | null>(null);
    const assistantId = currentAssistant?.assistant.id;
    const assistantType = currentAssistant?.assistant.assistant_type ?? 0;

    // Reset tab back to role when switching to a different assistant
    useEffect(() => {
        setActiveTab("role");
    }, [assistantId]);

    useEffect(() => {
        if (!mobile || assistantId == null) return;
        let active = true;
        invoke<{ available: boolean; issues: { capability: string; reason: string }[] }>(
            "get_assistant_execution_capability",
            { assistantId }
        )
            .then((result) => {
                if (active) {
                    setCompatibility({
                        id: assistantId,
                        reasons: result.issues
                            .filter((issue) => issue.capability === "plugin_assistant" || issue.capability === "local_cli_agent")
                            .map((issue) => issue.reason),
                    });
                }
            })
            .catch((error) => {
                if (active) setCompatibility({ id: assistantId, reasons: [`能力校验失败：${String(error)}`] });
            });
        return () => { active = false; };
    }, [mobile, assistantId]);

    const hasModelSelect = useMemo(
        () => formConfig.some((item) => item.config.type === "model-select"),
        [formConfig]
    );
    const hasProviderSelect = useMemo(
        () => formConfig.some((item) => item.config.type === "provider-select"),
        [formConfig]
    );
    const hasMcpSelect = useMemo(
        () => formConfig.some((item) => String(item.config.type) === "mcp-select"),
        [formConfig]
    );

    const { models, loading: modelsLoading, error: modelsError } = useFilteredModels(assistantType, hasModelSelect);
    const { providers, loading: providersLoading, error: providersError } = useFilteredProviders(assistantType, hasProviderSelect);
    const { mcpServers, loading: mcpServersLoading, error: mcpServersError } = useMcpServers(hasMcpSelect);
    const [defaultNewChatAssistantId, setDefaultNewChatAssistantId] = useState<number | null>(null);
    useEffect(() => {
        if (mobile) return;
        let active = true;
        invoke<number | null>("get_default_new_chat_assistant")
            .then((id) => {
                if (active) setDefaultNewChatAssistantId(id ?? null);
            })
            .catch(() => undefined);
        const sub = listen<number>("default-new-chat-assistant-changed", (event) => {
            if (active) setDefaultNewChatAssistantId(event.payload);
        });
        return () => {
            active = false;
            void sub.then((stop) => stop()).catch(() => undefined);
        };
    }, [mobile]);

    const fieldData = useMemo<FieldOptionsData>(
        () => ({
            models,
            modelsLoading,
            modelsError,
            providers,
            providersLoading,
            providersError,
            mcpServers,
            mcpServersLoading,
            mcpServersError,
        }),
        [models, modelsLoading, modelsError, providers, providersLoading, providersError, mcpServers, mcpServersLoading, mcpServersError]
    );

    const groupedFields = useMemo(() => {
        const groups = {
            role: [] as AssistantFormConfig[],
            tools: [] as AssistantFormConfig[],
            permissions: [] as AssistantFormConfig[],
            advancedBuiltin: [] as AssistantFormConfig[],
            advancedExtension: [] as AssistantFormConfig[],
        };
        for (const item of formConfig) {
            const target = classifyAssistantFieldTab(item);
            if (target === "advanced") {
                if (item.section === "extension") groups.advancedExtension.push(item);
                else groups.advancedBuiltin.push(item);
            } else {
                groups[target].push(item);
            }
        }
        return groups;
    }, [formConfig]);

    if (!currentAssistant) {
        return null;
    }
    if (isLoading) return <p role="status" className="p-6 text-sm text-muted-foreground">正在加载助手配置…</p>;

    if (mobile && compatibility?.id !== assistantId) {
        return <p role="status" className="p-6 text-sm text-muted-foreground">正在检查助手兼容性…</p>;
    }
    if (mobile && compatibility?.reasons.length) {
        return (
            <section className="space-y-3 p-4">
                <h2 className="text-lg font-semibold">{currentAssistant.assistant.name}</h2>
                <p className="text-sm text-muted-foreground">{currentAssistant.assistant.description}</p>
                <p className="text-sm">此助手在移动端只读，原配置已保留。请在桌面端编辑。</p>
                {compatibility.reasons.map((reason) => (
                    <p key={reason} role="status" className="text-xs text-muted-foreground">
                        {reason}
                    </p>
                ))}
            </section>
        );
    }

    const appearance =
        parseAppearance(featureConfig.get("butler_appearance")?.get(appearanceKey(currentAssistant.assistant.id))) ??
        defaultAppearance;
    const butlerEnabled = featureConfig.get("experimental")?.get("butler_experiment_enabled") === "true";
    const typeLabel =
        formConfig.find((item) => item.key === "assistantType")?.config.value ??
        (assistantType === 4 ? "智能体助手" : "普通对话助手");
    const agentProviderId =
        assistantType === 4
            ? String(
                currentAssistant.model[0]?.provider_id ||
                currentAssistant.model_configs.find((c) => c.name === "acp_provider")?.value ||
                ""
            )
            : "";

    const renderField = (item: AssistantFormConfig) => (
        <ConfigFormField
            key={item.key}
            field={item.config as any}
            name={item.key}
            control={form.control}
            data={fieldData}
            assistantConfigApi={assistantConfigApi}
        />
    );

    const roleNonPrompt = groupedFields.role.filter((item) => item.key !== "prompt" && item.key !== "assistantType");
    const promptItem = groupedFields.role.find((item) => item.key === "prompt");

    return (
        <Card className="bottom-space border-border shadow-none" data-aipp-config-section>
            {/* 身份头栏 */}
            <CardHeader className="border-b border-border/60 pb-4">
                <div className="flex flex-wrap items-start justify-between gap-4">
                    <div className="flex min-w-0 items-center gap-3.5">
                        <button
                            type="button"
                            onClick={onEdit}
                            disabled={!onEdit}
                            aria-label="编辑助手形象"
                            title="编辑名字、描述与形象"
                            className="flex h-16 w-16 shrink-0 items-center justify-center rounded-xl bg-muted/40 transition-colors hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
                        >
                            <ButlerAvatar id={currentAssistant.assistant.id} size={54} appearance={appearance} />
                        </button>
                        <div className="min-w-0 space-y-1">
                            <div className="flex flex-wrap items-center gap-2">
                                <h2 className="truncate text-lg font-semibold text-foreground">
                                    {currentAssistant.assistant.name}
                                </h2>
                                <span className="shrink-0 rounded-md bg-muted px-2 py-0.5 text-[11px] font-medium text-muted-foreground">
                                    {String(typeLabel)}
                                </span>
                                {isDirty ? (
                                    <span
                                        className="rounded-full border border-foreground/30 bg-muted px-2 py-0.5 text-xs font-medium text-foreground"
                                        data-aipp-slot="config-form-dirty-badge"
                                    >
                                        未保存
                                    </span>
                                ) : null}
                            </div>
                            <p className="line-clamp-2 text-sm text-muted-foreground">
                                {currentAssistant.assistant.description || "配置助手的角色、工具、权限与模型参数"}
                            </p>
                        </div>
                    </div>

                    <div className="flex shrink-0 items-center gap-1.5">
                        {!mobile && (
                            <Button
                                type="button"
                                variant="ghost"
                                size="sm"
                                disabled={defaultNewChatAssistantId === currentAssistant.assistant.id}
                                onClick={() => {
                                    void invoke("set_default_new_chat_assistant", { assistantId: currentAssistant.assistant.id })
                                        .then(() => {
                                            setDefaultNewChatAssistantId(currentAssistant.assistant.id);
                                            toast.success("已设为新对话默认助手");
                                        })
                                        .catch(error => toast.error(`保存新对话默认助手失败：${String(error)}`));
                                }}
                            >
                                {defaultNewChatAssistantId === currentAssistant.assistant.id ? "当前新对话默认" : "设为新对话默认"}
                            </Button>
                        )}
                        {onEdit ? (
                            <Button type="button" variant="outline" size="sm" onClick={onEdit} className="gap-1.5">
                                <Pencil className="h-3.5 w-3.5" />
                                编辑资料与形象
                            </Button>
                        ) : null}
                        {mobile && (onCopy || onDelete || onShare) ? (
                            <DropdownMenu>
                                <DropdownMenuTrigger asChild>
                                    <Button type="button" variant="ghost" size="icon" aria-label="更多操作">
                                        <MoreHorizontal className="h-4 w-4" />
                                    </Button>
                                </DropdownMenuTrigger>
                                <DropdownMenuContent align="end">
                                    {onCopy ? (
                                        <DropdownMenuItem onSelect={onCopy}>
                                            <Copy className="h-4 w-4" />
                                            复制助手
                                        </DropdownMenuItem>
                                    ) : null}
                                    {onShare ? (
                                        <DropdownMenuItem onSelect={onShare}>
                                            <Share className="h-4 w-4" />
                                            分享助手
                                        </DropdownMenuItem>
                                    ) : null}
                                    {onDelete ? (
                                        <DropdownMenuItem variant="destructive" onSelect={onDelete}>
                                            <Trash2 className="h-4 w-4" />
                                            删除助手
                                        </DropdownMenuItem>
                                    ) : null}
                                </DropdownMenuContent>
                            </DropdownMenu>
                        ) : (
                            <>
                                {onCopy ? (
                                    <Button iconLabel="复制" variant="ghost" size="sm" onClick={onCopy}>
                                        <Copy className="h-4 w-4" />
                                    </Button>
                                ) : null}
                                {onShare ? (
                                    <Button iconLabel="分享助手" variant="ghost" size="sm" onClick={onShare}>
                                        <Share className="h-4 w-4" />
                                    </Button>
                                ) : null}
                                {onDelete ? (
                                    <Button
                                        iconLabel="删除"
                                        variant="ghost"
                                        size="sm"
                                        onClick={onDelete}
                                        className="hover:bg-destructive/10 hover:text-destructive"
                                    >
                                        <Trash2 className="h-4 w-4" />
                                    </Button>
                                ) : null}
                            </>
                        )}
                    </div>
                </div>
            </CardHeader>

            <CardContent className="pt-4">
                <Form {...form}>
                    <Tabs value={activeTab} onValueChange={(v) => setActiveTab(v as AssistantTabId)} className="space-y-5">
                        <TabsList className="flex h-auto w-full flex-wrap justify-start gap-1 bg-muted/60 p-1">
                            <TabsTrigger value="role" className="gap-1.5 text-xs sm:text-sm">
                                <UserCog className="h-3.5 w-3.5" />
                                角色与模型
                            </TabsTrigger>
                            <TabsTrigger value="tools" className="gap-1.5 text-xs sm:text-sm">
                                <Wrench className="h-3.5 w-3.5" />
                                能力与工具
                            </TabsTrigger>
                            <TabsTrigger value="permissions" className="gap-1.5 text-xs sm:text-sm">
                                <Shield className="h-3.5 w-3.5" />
                                权限与工作区
                            </TabsTrigger>
                            <TabsTrigger value="advanced" className="gap-1.5 text-xs sm:text-sm">
                                <Sliders className="h-3.5 w-3.5" />
                                参数与高级
                            </TabsTrigger>
                            <TabsTrigger value="memory" className="gap-1.5 text-xs sm:text-sm">
                                <Brain className="h-3.5 w-3.5" />
                                记忆
                            </TabsTrigger>
                        </TabsList>

                        {/* Tab 1: 角色与模型 */}
                        <TabsContent
                            value="role"
                            forceMount
                            className={`mt-0 space-y-6 ${activeTab !== "role" ? "hidden" : ""}`}
                        >
                            <div className="grid grid-cols-1 gap-6 lg:grid-cols-2">
                                <div className="space-y-5">
                                    {roleNonPrompt.map(renderField)}
                                </div>
                                {promptItem ? (
                                    <div className="space-y-5">{renderField(promptItem)}</div>
                                ) : null}
                            </div>
                        </TabsContent>

                        {/* Tab 2: 能力与工具 */}
                        <TabsContent
                            value="tools"
                            forceMount
                            className={`mt-0 space-y-5 ${activeTab !== "tools" ? "hidden" : ""}`}
                        >
                            {butlerEnabled ? (
                                <div className="flex items-start gap-2.5 rounded-lg border border-border bg-muted/30 px-3.5 py-2.5 text-xs text-muted-foreground">
                                    <Info className="mt-0.5 h-4 w-4 shrink-0 text-foreground/70" />
                                    <span>
                                        以下 MCP 与 Skills 配置作用于普通聊天。在<strong>总管家模式</strong>（联系人私聊、群聊、受托任务）中，助手会自动获得全局启用的 MCP 与 Skills，不受此处配置限制。
                                    </span>
                                </div>
                            ) : null}
                            {groupedFields.tools.length === 0 ? (
                                <p className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
                                    当前助手类型暂无可单独配置的工具项。
                                </p>
                            ) : (
                                <div className="space-y-5">
                                    {groupedFields.tools.map(renderField)}
                                </div>
                            )}
                        </TabsContent>

                        {/* Tab 3: 权限与工作区 */}
                        <TabsContent
                            value="permissions"
                            forceMount
                            className={`mt-0 space-y-5 ${activeTab !== "permissions" ? "hidden" : ""}`}
                        >
                            {assistantType === 4 && agentProviderId && agentProviderId !== "-1" ? (
                                <div className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border bg-muted/30 px-3.5 py-2.5 text-xs text-muted-foreground">
                                    <span>
                                        智能体助手的审批与沙箱默认继承所选提供商的全局配置；如需修改全局默认值，可前往提供商页调整。
                                    </span>
                                    <SettingsLink
                                        menu="llm-provider-config"
                                        subNav={agentProviderId}
                                        navigateTo={navigateTo}
                                        variant="link"
                                    >
                                        查看提供商设置
                                    </SettingsLink>
                                </div>
                            ) : null}
                            {groupedFields.permissions.length === 0 ? (
                                <p className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
                                    当前助手暂无额外权限或工作区设置。
                                </p>
                            ) : (
                                <div className="space-y-5">
                                    {groupedFields.permissions.map(renderField)}
                                </div>
                            )}
                        </TabsContent>

                        {/* Tab 4: 参数与高级 */}
                        <TabsContent
                            value="advanced"
                            forceMount
                            className={`mt-0 space-y-6 ${activeTab !== "advanced" ? "hidden" : ""}`}
                        >
                            {groupedFields.advancedBuiltin.length > 0 ? (
                                <div className="space-y-4">
                                    <h3 className="text-sm font-medium text-foreground">
                                        {assistantType === 4 ? "运行环境与启动参数" : "模型采样与运行参数"}
                                    </h3>
                                    <div className="grid grid-cols-1 gap-x-8 gap-y-5 lg:grid-cols-2">
                                        {groupedFields.advancedBuiltin.map(renderField)}
                                    </div>
                                </div>
                            ) : null}

                            {groupedFields.advancedExtension.length > 0 ? (
                                <div className="space-y-4 border-t border-border pt-5" data-aipp-slot="assistant-extensions">
                                    <div>
                                        <h3 className="text-sm font-medium text-foreground">插件扩展配置</h3>
                                        <p className="text-xs text-muted-foreground">配置已安装插件为该助手提供的扩展项。</p>
                                    </div>
                                    <div className="grid grid-cols-1 gap-x-8 gap-y-5 lg:grid-cols-2">
                                        {groupedFields.advancedExtension.map(renderField)}
                                    </div>
                                </div>
                            ) : null}

                            {groupedFields.advancedBuiltin.length === 0 && groupedFields.advancedExtension.length === 0 ? (
                                <p className="rounded-lg border border-dashed px-4 py-6 text-center text-sm text-muted-foreground">
                                    当前助手暂无可调高级参数或插件扩展项。
                                </p>
                            ) : null}
                        </TabsContent>

                        {/* Tab 5: 记忆 */}
                        <TabsContent value="memory" className="mt-0">
                            {activeTab === "memory" ? (
                                <ButlerMemoryPanel
                                    assistantId={currentAssistant.assistant.id}
                                    scope="assistant"
                                />
                            ) : null}
                        </TabsContent>
                    </Tabs>

                    {activeTab !== "memory" ? (
                        <ConfigSaveFooter>
                            <div className="flex items-center justify-between gap-3">
                                <div className="flex items-center gap-2">
                                    <Button
                                        type="button"
                                        disabled={!isDirty || isSaving}
                                        onClick={onSave}
                                        className="bg-primary text-primary-foreground hover:bg-primary/90"
                                    >
                                        {isSaving ? "保存中…" : "保存配置"}
                                    </Button>
                                    {isDirty && onDiscard ? (
                                        <Button
                                            type="button"
                                            variant="outline"
                                            disabled={isSaving}
                                            onClick={onDiscard}
                                        >
                                            放弃更改
                                        </Button>
                                    ) : null}
                                </div>
                                {isDirty ? (
                                    <span className="text-xs font-medium text-foreground">有未保存的更改</span>
                                ) : null}
                            </div>
                        </ConfigSaveFooter>
                    ) : null}
                </Form>
            </CardContent>
        </Card>
    );
};
