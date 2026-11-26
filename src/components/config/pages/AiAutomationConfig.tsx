import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Progress } from "@/components/ui/progress";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Textarea } from "@/components/ui/textarea";
import { ConfigPageLoading } from "@/components/common";
import { ConfigSaveFooter } from "@/components/common/ConfigSaveFooter";
import { DecisionModelProbeButton } from "@/components/config/DecisionModelProbeButton";
import { ReviewOutputCapability } from "@/components/config/feature/forms/SummaryConfigForm";
import { SettingsGroup, SettingRowText, settingRowClassName, stackedRowClassName } from "@/components/config/SettingsGroup";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import { localLightWarning } from "@/components/onboarding/onboardingService";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import { useModels } from "@/hooks/useModels";
import { useUnsavedLeaveGuard } from "@/hooks/useUnsavedLeaveGuard";
import { parseSubNav, useSettingsAnchorHighlight } from "@/lib/settingsNavigation";
import { getErrorMessage } from "@/utils/error";
import { isDecisionRequestMode } from "@/components/config/llmModelTypes";
import SettingsPageShell from "./SettingsPageShell";

const INHERIT_VALUE = "__inherit__";

const SUMMARY_LENGTH_OPTIONS = [50, 100, 300, 500, 1000, -1].map((m) => ({
    value: m.toString(),
    label: m === -1 ? "所有" : m.toString(),
}));

interface MCPSummaryProgressPayload {
    phase: "started" | "processing" | "progress" | "completed" | "interrupted" | "failed";
    updated_at?: string;
    failures?: string[];
    total: number;
    completed: number;
    succeeded: number;
    failed: number;
    server_name?: string;
    message?: string;
}

interface ExperimentalSummaryTaskStatus {
    mcp_progress?: MCPSummaryProgressPayload;
    mcp_running: boolean;
    conversation_running: boolean;
    conversation_running_count: number;
}

interface AiAutomationConfigProps {
    subNav?: string;
    onSubNavConsumed?: () => void;
}

const parseModelPair = (modelValue: string) => {
    if (!modelValue) return { model_code: "", provider_id: "" };
    const sep = modelValue.lastIndexOf("%%");
    if (sep <= 0) return { model_code: "", provider_id: "" };
    return { model_code: modelValue.slice(0, sep), provider_id: modelValue.slice(sep + 2) };
};

export const AiAutomationConfig: React.FC<AiAutomationConfigProps> = ({
    subNav,
    onSubNavConsumed,
}) => {
    const { featureConfig, saveFeatureConfig, loading } = useFeatureConfig();
    const [initialLoaded, setInitialLoaded] = useState(false);
    const [saving, setSaving] = useState(false);
    const [anchor, setAnchor] = useState<string | null>(null);

    // 系统默认辅助模型
    const [defaultLightModel, setDefaultLightModel] = useState("");
    const [defaultDecisionModel, setDefaultDecisionModel] = useState("");

    // 工具自动审核
    const [autoReviewModel, setAutoReviewModel] = useState("");
    const [autoReviewOutputMode, setAutoReviewOutputMode] = useState("auto");
    const [autoReviewDecisionThreshold, setAutoReviewDecisionThreshold] = useState("0.90");

    // 工具加载与执行策略
    const [dynamicMcpEnabled, setDynamicMcpEnabled] = useState(false);
    const [mcpSummarizerModel, setMcpSummarizerModel] = useState("");
    const [toolErrorContinue, setToolErrorContinue] = useState(true);

    // 上下文自动压缩
    const [contextCompactionEnabled, setContextCompactionEnabled] = useState(true);
    const [contextMaxInputTokens, setContextMaxInputTokens] = useState("128000");
    const [contextCompactionThreshold, setContextCompactionThreshold] = useState("0.80");
    const [contextTailRatio, setContextTailRatio] = useState("0.30");

    // 对话标题生成
    const [titleSummaryEnabled, setTitleSummaryEnabled] = useState(true);
    const [titleModel, setTitleModel] = useState("");
    const [titleSummaryLength, setTitleSummaryLength] = useState("100");
    const [titlePrompt, setTitlePrompt] = useState("");

    // 空闲对话自动总结
    const [conversationSummaryEnabled, setConversationSummaryEnabled] = useState(false);
    const [conversationSummaryModel, setConversationSummaryModel] = useState("");

    // 表单自动填写
    const [formAutofillEnabled, setFormAutofillEnabled] = useState(true);
    const [formAutofillModel, setFormAutofillModel] = useState("");

    const [summaryProgress, setSummaryProgress] = useState<MCPSummaryProgressPayload | null>(null);
    const [summaryTasks, setSummaryTasks] = useState<ExperimentalSummaryTaskStatus>({
        mcp_running: false,
        conversation_running: false,
        conversation_running_count: 0,
    });
    const [runningAction, setRunningAction] = useState<{ mcp: boolean; conversation: boolean }>({
        mcp: false,
        conversation: false,
    });

    const baselineRef = useRef<string>("");

    const { models: lightModels, loading: lightLoading } = useModels(true);
    const { models: decisionModels, loading: decisionLoading } = useModels(true, { includeDecision: true });

    const { models: agentModels } = useFilteredModels(4);
    const lightOptions = useMemo(
        () => [...lightModels, ...agentModels.filter(m => m.id > 0)].map((m) => ({ value: `${m.code}%%${m.llm_provider_id}`, label: m.name })),
        [agentModels, lightModels]
    );
    const decisionOptions = useMemo(
        () =>
            decisionModels.map((m) => ({
                value: `${m.code}%%${m.llm_provider_id}`,
                label: m.is_decision || isDecisionRequestMode(m.request_mode) ? `${m.name} [决策]` : m.name,
            })),
        [decisionModels]
    );

    const lightModelLabel = useMemo(
        () => lightOptions.find((o) => o.value === defaultLightModel)?.label,
        [lightOptions, defaultLightModel]
    );
    const decisionModelLabel = useMemo(
        () => decisionOptions.find((o) => o.value === defaultDecisionModel)?.label,
        [decisionOptions, defaultDecisionModel]
    );

    const effectiveReviewModel = autoReviewModel || defaultDecisionModel;
    const isDecisionReviewModel = useMemo(() => {
        if (!effectiveReviewModel) return false;
        const found = decisionModels.find(
            (m) => `${m.code}%%${m.llm_provider_id}` === effectiveReviewModel
        );
        return Boolean(found && (found.is_decision || isDecisionRequestMode(found.request_mode)));
    }, [decisionModels, effectiveReviewModel]);

    useEffect(() => {
        if (!subNav) return;
        const parsed = parseSubNav(subNav);
        if (parsed.anchor) setAnchor(parsed.anchor);
        onSubNavConsumed?.();
    }, [subNav, onSubNavConsumed]);

    useSettingsAnchorHighlight(anchor, () => setAnchor(null), String(initialLoaded));

    const loadTaskStatus = useCallback(async () => {
        try {
            const status = await invoke<ExperimentalSummaryTaskStatus>("get_experimental_summary_task_status");
            if (status.mcp_progress) {
                setSummaryProgress((cur) =>
                    cur?.updated_at &&
                    status.mcp_progress?.updated_at &&
                    cur.updated_at > status.mcp_progress.updated_at
                        ? cur
                        : status.mcp_progress!
                );
            }
            setSummaryTasks(status);
        } catch {
            // ignore background status errors
        }
    }, []);

    useEffect(() => {
        void loadTaskStatus();
        const unlisten = listen<MCPSummaryProgressPayload>("mcp-summary-progress", (event) => {
            setSummaryProgress(event.payload);
            setSummaryTasks((cur) => ({
                ...cur,
                mcp_running: !["failed", "completed", "interrupted"].includes(event.payload.phase),
            }));
        });
        const timer = window.setInterval(() => void loadTaskStatus(), 5000);
        return () => {
            window.clearInterval(timer);
            void unlisten.then((fn) => fn());
        };
    }, [loadTaskStatus]);

    const hydrateFromConfig = useCallback(() => {
        const sc = featureConfig.get("conversation_summary") ?? new Map<string, string>();
        const ec = featureConfig.get("experimental") ?? new Map<string, string>();
        const tec = featureConfig.get("tool_error_continue") ?? new Map<string, string>();

        const rawTitleModel = sc.get("title_model") || sc.get("model_code") || "";
        const rawTitleProvider = sc.get("title_provider_id") || sc.get("provider_id") || "";
        const rawAutoReviewModel = sc.get("auto_review_model") || "";
        const rawAutoReviewProvider = sc.get("auto_review_provider_id") || "";

        const nextState = {
            defaultLightModel: sc.get("default_light_model") || "",
            defaultDecisionModel: sc.get("default_decision_model") || "",
            autoReviewModel:
                rawAutoReviewModel && rawAutoReviewProvider
                    ? `${rawAutoReviewModel}%%${rawAutoReviewProvider}`
                    : "",
            autoReviewOutputMode: sc.get("auto_review_output_mode") || "auto",
            autoReviewDecisionThreshold: sc.get("auto_review_decision_threshold") || "0.90",
            dynamicMcpEnabled: ec.get("dynamic_mcp_loading_enabled") === "true",
            mcpSummarizerModel: ec.get("mcp_summarizer_model_id") || "",
            toolErrorContinue: (tec.get("enabled") ?? "true") !== "false" && (tec.get("enabled") ?? "true") !== "0",
            contextCompactionEnabled: ["true", "1"].includes(ec.get("context_compaction_enabled") ?? "true"),
            contextMaxInputTokens: ec.get("context_max_input_tokens") || "128000",
            contextCompactionThreshold: ec.get("context_compaction_threshold") || "0.80",
            contextTailRatio: ec.get("context_tail_ratio") || "0.30",
            titleSummaryEnabled: sc.get("title_summary_enabled") !== "false",
            titleModel:
                rawTitleModel && rawTitleProvider ? `${rawTitleModel}%%${rawTitleProvider}` : "",
            titleSummaryLength: sc.get("title_summary_length") || sc.get("summary_length") || "100",
            titlePrompt: sc.get("title_prompt") || sc.get("prompt") || "",
            formAutofillEnabled: sc.get("form_autofill_enabled") !== "false",
            formAutofillModel: sc.get("form_autofill_model") || "",
            conversationSummaryEnabled: false,
            conversationSummaryModel: "",
        };

        const convSource = [
            "conversation_summary_enabled",
            "conversation_summary_model",
            "conversation_summary_provider_id",
        ].some((k) => ec.has(k))
            ? ec
            : sc;
        nextState.conversationSummaryEnabled = convSource.get("conversation_summary_enabled") === "true";
        const convM = convSource.get("conversation_summary_model") || "";
        const convP = convSource.get("conversation_summary_provider_id") || "";
        nextState.conversationSummaryModel =
            convM && convP ? `${convM}%%${convP}` : convM.includes("%%") ? convM : "";

        setDefaultLightModel(nextState.defaultLightModel);
        setDefaultDecisionModel(nextState.defaultDecisionModel);
        setAutoReviewModel(nextState.autoReviewModel);
        setAutoReviewOutputMode(nextState.autoReviewOutputMode);
        setAutoReviewDecisionThreshold(nextState.autoReviewDecisionThreshold);
        setDynamicMcpEnabled(nextState.dynamicMcpEnabled);
        setMcpSummarizerModel(nextState.mcpSummarizerModel);
        setToolErrorContinue(nextState.toolErrorContinue);
        setContextCompactionEnabled(nextState.contextCompactionEnabled);
        setContextMaxInputTokens(nextState.contextMaxInputTokens);
        setContextCompactionThreshold(nextState.contextCompactionThreshold);
        setContextTailRatio(nextState.contextTailRatio);
        setTitleSummaryEnabled(nextState.titleSummaryEnabled);
        setTitleModel(nextState.titleModel);
        setTitleSummaryLength(nextState.titleSummaryLength);
        setTitlePrompt(nextState.titlePrompt);
        setFormAutofillEnabled(nextState.formAutofillEnabled);
        setFormAutofillModel(nextState.formAutofillModel);
        setConversationSummaryEnabled(nextState.conversationSummaryEnabled);
        setConversationSummaryModel(nextState.conversationSummaryModel);

        baselineRef.current = JSON.stringify(nextState);
    }, [featureConfig]);

    useEffect(() => {
        if (loading) return;
        hydrateFromConfig();
        setInitialLoaded(true);
    }, [loading, hydrateFromConfig]);

    const currentSnapshot = JSON.stringify({
        defaultLightModel,
        defaultDecisionModel,
        autoReviewModel,
        autoReviewOutputMode,
        autoReviewDecisionThreshold,
        dynamicMcpEnabled,
        mcpSummarizerModel,
        toolErrorContinue,
        contextCompactionEnabled,
        contextMaxInputTokens,
        contextCompactionThreshold,
        contextTailRatio,
        titleSummaryEnabled,
        titleModel,
        titleSummaryLength,
        titlePrompt,
        formAutofillEnabled,
        formAutofillModel,
        conversationSummaryEnabled,
        conversationSummaryModel,
    });
    const isDirty = initialLoaded && currentSnapshot !== baselineRef.current;

    const handleSaveAll = useCallback(async () => {
        setSaving(true);
        try {
            const parsedTitle = parseModelPair(titleModel);
            const parsedAutofill = parseModelPair(formAutofillModel);
            const parsedReview = parseModelPair(autoReviewModel);
            const parsedConv = parseModelPair(conversationSummaryModel);

            if (titleSummaryEnabled && !parsedTitle.provider_id && !defaultLightModel) {
                throw new Error("请为对话标题生成选择模型，或在顶部配置「默认轻量模型」");
            }
            if (conversationSummaryEnabled && !parsedConv.provider_id && !defaultLightModel) {
                throw new Error("请为空闲对话总结选择模型，或在顶部配置「默认轻量模型」");
            }

            await saveFeatureConfig(
                "conversation_summary",
                {
                    assistant_ai_enabled: "true",
                    default_light_model: defaultLightModel,
                    default_decision_model: defaultDecisionModel,
                    title_summary_enabled: String(titleSummaryEnabled),
                    title_model: parsedTitle.model_code,
                    title_provider_id: parsedTitle.provider_id,
                    title_summary_length: titleSummaryLength,
                    title_prompt: titlePrompt,
                    form_autofill_enabled: String(formAutofillEnabled),
                    form_autofill_model: formAutofillModel,
                    form_autofill_provider_id: parsedAutofill.provider_id,
                    auto_review_model: parsedReview.model_code,
                    auto_review_provider_id: parsedReview.provider_id,
                    auto_review_output_mode: autoReviewOutputMode,
                    auto_review_decision_threshold: String(autoReviewDecisionThreshold || "0.90"),
                },
                true
            );

            await saveFeatureConfig(
                "experimental",
                {
                    context_compaction_enabled: String(contextCompactionEnabled),
                    context_max_input_tokens: String(contextMaxInputTokens || "128000"),
                    context_compaction_threshold: String(contextCompactionThreshold || "0.80"),
                    context_tail_ratio: String(contextTailRatio || "0.30"),
                    dynamic_mcp_loading_enabled: String(dynamicMcpEnabled),
                    mcp_summarizer_model_id: mcpSummarizerModel,
                    conversation_summary_enabled: String(conversationSummaryEnabled),
                    conversation_summary_model: parsedConv.model_code,
                    conversation_summary_provider_id: parsedConv.provider_id,
                },
                true
            );

            await saveFeatureConfig("tool_error_continue", {
                enabled: toolErrorContinue ? "true" : "false",
            });

            baselineRef.current = currentSnapshot;
            toast.success("AI 与自动化配置已保存");
        } catch (err) {
            toast.error("保存配置失败：" + getErrorMessage(err));
            throw err;
        } finally {
            setSaving(false);
        }
    }, [
        titleModel,
        formAutofillModel,
        autoReviewModel,
        conversationSummaryModel,
        titleSummaryEnabled,
        defaultLightModel,
        conversationSummaryEnabled,
        saveFeatureConfig,
        defaultDecisionModel,
        titleSummaryLength,
        titlePrompt,
        formAutofillEnabled,
        autoReviewOutputMode,
        autoReviewDecisionThreshold,
        contextCompactionEnabled,
        contextMaxInputTokens,
        contextCompactionThreshold,
        contextTailRatio,
        dynamicMcpEnabled,
        mcpSummarizerModel,
        toolErrorContinue,
        currentSnapshot,
    ]);

    const { dialog: leaveDialog } = useUnsavedLeaveGuard({
        isDirty,
        onSave: handleSaveAll,
        onDiscard: hydrateFromConfig,
    });

    const handleTriggerTask = async (kind: "mcp" | "conversation") => {
        setRunningAction((cur) => ({ ...cur, [kind]: true }));
        try {
            const cmd =
                kind === "mcp"
                    ? "trigger_mcp_summary_generation"
                    : "trigger_conversation_summary_generation";
            const res = await invoke<{ started: boolean; message: string }>(cmd);
            await loadTaskStatus();
            if (res.started) toast.success(res.message);
            else toast.info(res.message);
        } catch (err) {
            toast.error("启动任务失败：" + getErrorMessage(err));
        } finally {
            setRunningAction((cur) => ({ ...cur, [kind]: false }));
        }
    };

    if (!initialLoaded) {
        return <ConfigPageLoading />;
    }

    const progressValue =
        summaryProgress && summaryProgress.total > 0
            ? Math.round((summaryProgress.completed / summaryProgress.total) * 100)
            : 0;

    return (
        <>
            <SettingsPageShell
                title="AI 与自动化"
                description="统一配置系统默认辅助模型、工具自动审核、上下文压缩、MCP 动态加载与后台摘要任务。"
                headerExtra={
                    isDirty ? (
                        <span className="rounded-full border border-foreground/30 bg-muted px-2.5 py-0.5 text-xs font-medium">
                            未保存
                        </span>
                    ) : null
                }
            >
                {/* 1. 系统默认辅助模型 */}
                <SettingsGroup
                    id="default-models"
                    title="系统默认辅助模型"
                    description="只需在此配置一次，下方的标题生成、表单填写、对话总结、MCP 摘要、管家辅助与工具审核在未单独指定时均自动沿用。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="默认轻量模型"
                            description={agentModels.some(m => `${m.code}%%${m.llm_provider_id}` === defaultLightModel) ? localLightWarning : "用于标题生成、表单自动填写、对话总结、MCP 摘要、管家状态总结与主动整理初筛。建议选响应快、价格低的小模型。"}
                        />
                        <div className="w-64 shrink-0">
                            <Select
                                disabled={lightLoading}
                                value={defaultLightModel || INHERIT_VALUE}
                                onValueChange={(v) => setDefaultLightModel(v === INHERIT_VALUE ? "" : v)}
                            >
                                <SelectTrigger className="w-full" aria-label="默认轻量模型">
                                    <SelectValue placeholder={lightLoading ? "加载中…" : "未设置"} />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value={INHERIT_VALUE}>未设置</SelectItem>
                                    {lightOptions.map((o) => (
                                        <SelectItem key={o.value} value={o.value}>
                                            {o.label}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    </div>
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="默认决策 / 审核模型"
                            description="用于工具调用自动审核与管家群聊回复决策，支持普通对话模型与标有 [决策] 的专用决策模型。"
                        />
                        <div className="w-64 shrink-0">
                            <Select
                                disabled={decisionLoading}
                                value={defaultDecisionModel || INHERIT_VALUE}
                                onValueChange={(v) => setDefaultDecisionModel(v === INHERIT_VALUE ? "" : v)}
                            >
                                <SelectTrigger className="w-full" aria-label="默认决策与审核模型">
                                    <SelectValue placeholder={decisionLoading ? "加载中…" : "未设置"} />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value={INHERIT_VALUE}>未设置</SelectItem>
                                    {decisionOptions.map((o) => (
                                        <SelectItem key={o.value} value={o.value}>
                                            {o.label}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    </div>
                </SettingsGroup>

                {/* 2. 工具安全自动审核 */}
                <SettingsGroup
                    id="auto-review-model"
                    title="工具安全自动审核"
                    description="安全的内置只读操作直接放行；其他操作与外部工具按助手审核模式决定是否调用审核模型判断，审核中可随时人工介入。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="审核模型（按需覆写）"
                            description="支持普通对话模型与决策模型（标有 [决策]）。未配置且未设默认决策模型时，外部调用一律转为人工确认。"
                        />
                        <div className="w-64 shrink-0">
                            <Select
                                disabled={decisionLoading}
                                value={autoReviewModel || INHERIT_VALUE}
                                onValueChange={(v) => setAutoReviewModel(v === INHERIT_VALUE ? "" : v)}
                            >
                                <SelectTrigger className="w-full" aria-label="自动审核模型">
                                    <SelectValue />
                                </SelectTrigger>
                                <SelectContent>
                                    <SelectItem value={INHERIT_VALUE}>
                                        {decisionModelLabel
                                            ? `沿用默认决策/审核模型（${decisionModelLabel}）`
                                            : "沿用默认决策/审核模型（未设置）"}
                                    </SelectItem>
                                    {decisionOptions.map((o) => (
                                        <SelectItem key={o.value} value={o.value}>
                                            {o.label}
                                        </SelectItem>
                                    ))}
                                </SelectContent>
                            </Select>
                        </div>
                    </div>
                    {isDecisionReviewModel ? (
                        <>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="决策放行安全阈值"
                                    description="安全概率不低于此阈值时放行（0.50 ~ 0.99，默认 0.90）；高危分类始终转人工确认。"
                                />
                                <Input
                                    type="number"
                                    step="0.01"
                                    min="0.50"
                                    max="0.99"
                                    className="w-28 shrink-0"
                                    value={autoReviewDecisionThreshold}
                                    onChange={(e) => setAutoReviewDecisionThreshold(e.target.value)}
                                    placeholder="0.90"
                                />
                            </div>
                            <div className={stackedRowClassName}>
                                <DecisionModelProbeButton modelValue={effectiveReviewModel} />
                            </div>
                        </>
                    ) : (
                        <div className={stackedRowClassName}>
                            <div className="flex flex-wrap items-center justify-between gap-3">
                                <SettingRowText
                                    label="审核输出方式"
                                    description="自动模式优先使用 JSON Schema；不支持时回退 Prompt JSON 并记忆结果，保存后生效。"
                                />
                                <div className="w-56 shrink-0">
                                    <Select value={autoReviewOutputMode} onValueChange={setAutoReviewOutputMode}>
                                        <SelectTrigger className="w-full" aria-label="审核输出方式">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value="auto">自动（推荐）</SelectItem>
                                            <SelectItem value="schema">强制 Format（JSON Schema）</SelectItem>
                                            <SelectItem value="prompt">仅 Prompt（JSON）</SelectItem>
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <ReviewOutputCapability model={effectiveReviewModel} mode={autoReviewOutputMode} />
                        </div>
                    )}
                </SettingsGroup>

                {/* 3. 工具加载与执行策略 */}
                <SettingsGroup
                    id="dynamic-mcp"
                    title="工具加载与执行策略"
                    description="控制 MCP 工具按需加载、目录摘要生成以及工具出错时的对话延续策略。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText
                            label="MCP 动态加载"
                            description="开启后采用 MCP 目录摘要 + 按需加载模式，减少大量工具定义对上下文的占用。"
                        />
                        <Switch
                            aria-label="MCP 动态加载"
                            checked={dynamicMcpEnabled}
                            onCheckedChange={setDynamicMcpEnabled}
                        />
                    </div>
                    {dynamicMcpEnabled ? (
                        <div className={stackedRowClassName}>
                            <div className="flex flex-wrap items-center justify-between gap-3">
                                <SettingRowText
                                    label="MCP 摘要模型"
                                    description="目录应用优先用线上摘要，自定义 MCP 与缺失摘要由所选模型生成。"
                                />
                                <div className="w-64 shrink-0">
                                    <Select
                                        disabled={lightLoading}
                                        value={mcpSummarizerModel || INHERIT_VALUE}
                                        onValueChange={(v) => setMcpSummarizerModel(v === INHERIT_VALUE ? "" : v)}
                                    >
                                        <SelectTrigger className="w-full" aria-label="MCP 摘要模型">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value={INHERIT_VALUE}>
                                                {lightModelLabel
                                                    ? `沿用默认轻量模型（${lightModelLabel}）`
                                                    : "沿用默认轻量模型"}
                                            </SelectItem>
                                            {lightOptions.map((o) => (
                                                <SelectItem key={o.value} value={o.value}>
                                                    {o.label}
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border bg-muted/20 px-3 py-2">
                                <span className="text-xs text-muted-foreground">
                                    {summaryProgress
                                        ? summaryProgress.phase === "completed"
                                            ? `上次完成：成功 ${summaryProgress.succeeded}，失败 ${summaryProgress.failed}`
                                            : summaryProgress.server_name
                                                ? `处理中：${summaryProgress.server_name} (${summaryProgress.completed}/${summaryProgress.total})`
                                                : summaryProgress.message || "后台就绪"
                                        : "保存后或手动点击触发后台生成"}
                                </span>
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    disabled={
                                        saving ||
                                        runningAction.mcp ||
                                        summaryTasks.mcp_running ||
                                        (!mcpSummarizerModel && !defaultLightModel)
                                    }
                                    onClick={() => void handleTriggerTask("mcp")}
                                >
                                    {summaryTasks.mcp_running ? "总结进行中…" : "立即生成 MCP 摘要"}
                                </Button>
                            </div>
                            {summaryTasks.mcp_running ? <Progress value={progressValue} /> : null}
                        </div>
                    ) : null}
                    <div id="tool-error-continue" className={settingRowClassName}>
                        <SettingRowText
                            label="工具错误后继续对话"
                            description="开启后，自动运行的工具执行出错时会把错误信息返回给模型继续处理，而不是中断回合。"
                        />
                        <Switch
                            aria-label="工具错误后继续对话"
                            checked={toolErrorContinue}
                            onCheckedChange={setToolErrorContinue}
                        />
                    </div>
                </SettingsGroup>

                {/* 4. 上下文自动压缩 */}
                <SettingsGroup
                    id="context-compaction"
                    title="上下文自动压缩"
                    description="为长对话自动压缩较早历史，适用于使用普通聊天链路的对话。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText label="启用上下文自动压缩" />
                        <Switch
                            aria-label="启用上下文自动压缩"
                            checked={contextCompactionEnabled}
                            onCheckedChange={setContextCompactionEnabled}
                        />
                    </div>
                    {contextCompactionEnabled ? (
                        <>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="模型上下文窗口"
                                    description="模型的上下文窗口总大小（含输入与输出），系统会自动预留输出空间。"
                                />
                                <Input
                                    type="number"
                                    className="w-36 shrink-0"
                                    value={contextMaxInputTokens}
                                    onChange={(e) => setContextMaxInputTokens(e.target.value)}
                                    placeholder="128000"
                                />
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="压缩触发比例"
                                    description="估算 Token 使用量达到此比例时触发自动压缩（0.50 ~ 0.99）。"
                                />
                                <Input
                                    type="number"
                                    step="0.05"
                                    min="0.5"
                                    max="0.99"
                                    className="w-28 shrink-0"
                                    value={contextCompactionThreshold}
                                    onChange={(e) => setContextCompactionThreshold(e.target.value)}
                                    placeholder="0.80"
                                />
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText
                                    label="尾部保留比例"
                                    description="为最近消息保留的上下文预算比例（0.05 ~ 0.80），从最新消息往前保留。"
                                />
                                <Input
                                    type="number"
                                    step="0.05"
                                    min="0.05"
                                    max="0.80"
                                    className="w-28 shrink-0"
                                    value={contextTailRatio}
                                    onChange={(e) => setContextTailRatio(e.target.value)}
                                    placeholder="0.30"
                                />
                            </div>
                        </>
                    ) : null}
                </SettingsGroup>

                {/* 5. 对话标题生成 */}
                <SettingsGroup
                    id="title-summary"
                    title="对话标题生成"
                    description="对话开始时自动总结首轮内容并生成标题。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText label="自动生成对话标题" />
                        <Switch
                            aria-label="自动生成对话标题"
                            checked={titleSummaryEnabled}
                            onCheckedChange={setTitleSummaryEnabled}
                        />
                    </div>
                    {titleSummaryEnabled ? (
                        <>
                            <div className={settingRowClassName}>
                                <SettingRowText label="标题生成模型" />
                                <div className="w-64 shrink-0">
                                    <Select
                                        disabled={lightLoading}
                                        value={titleModel || INHERIT_VALUE}
                                        onValueChange={(v) => setTitleModel(v === INHERIT_VALUE ? "" : v)}
                                    >
                                        <SelectTrigger className="w-full" aria-label="标题生成模型">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value={INHERIT_VALUE}>
                                                {lightModelLabel
                                                    ? `沿用默认轻量模型（${lightModelLabel}）`
                                                    : "沿用默认轻量模型"}
                                            </SelectItem>
                                            {lightOptions.map((o) => (
                                                <SelectItem key={o.value} value={o.value}>
                                                    {o.label}
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <div className={settingRowClassName}>
                                <SettingRowText label="参考文本长度" description="截取对话开头多少字符用于总结标题。" />
                                <div className="w-36 shrink-0">
                                    <Select value={titleSummaryLength} onValueChange={setTitleSummaryLength}>
                                        <SelectTrigger className="w-full" aria-label="参考文本长度">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            {SUMMARY_LENGTH_OPTIONS.map((o) => (
                                                <SelectItem key={o.value} value={o.value}>
                                                    {o.label}
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <div className={stackedRowClassName}>
                                <SettingRowText label="总结 Prompt" description="留空则使用系统默认标题生成提示词。" />
                                <Textarea
                                    className="min-h-28 text-sm"
                                    value={titlePrompt}
                                    onChange={(e) => setTitlePrompt(e.target.value)}
                                    placeholder="可选：自定义标题生成提示词"
                                />
                            </div>
                        </>
                    ) : null}
                </SettingsGroup>

                {/* 6. 空闲对话自动总结 */}
                <SettingsGroup
                    id="conversation-summary"
                    title="空闲对话自动总结"
                    description="对话空闲一段时间后自动在后台生成摘要，用于历史检索、回顾与上下文重建。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText label="启用空闲对话自动总结" />
                        <Switch
                            aria-label="启用空闲对话自动总结"
                            checked={conversationSummaryEnabled}
                            onCheckedChange={setConversationSummaryEnabled}
                        />
                    </div>
                    {conversationSummaryEnabled ? (
                        <div className={stackedRowClassName}>
                            <div className="flex flex-wrap items-center justify-between gap-3">
                                <SettingRowText label="对话总结模型" />
                                <div className="w-64 shrink-0">
                                    <Select
                                        disabled={lightLoading}
                                        value={conversationSummaryModel || INHERIT_VALUE}
                                        onValueChange={(v) =>
                                            setConversationSummaryModel(v === INHERIT_VALUE ? "" : v)
                                        }
                                    >
                                        <SelectTrigger className="w-full" aria-label="对话总结模型">
                                            <SelectValue />
                                        </SelectTrigger>
                                        <SelectContent>
                                            <SelectItem value={INHERIT_VALUE}>
                                                {lightModelLabel
                                                    ? `沿用默认轻量模型（${lightModelLabel}）`
                                                    : "沿用默认轻量模型"}
                                            </SelectItem>
                                            {lightOptions.map((o) => (
                                                <SelectItem key={o.value} value={o.value}>
                                                    {o.label}
                                                </SelectItem>
                                            ))}
                                        </SelectContent>
                                    </Select>
                                </div>
                            </div>
                            <div className="flex flex-wrap items-center justify-between gap-2 rounded-md border bg-muted/20 px-3 py-2">
                                <span className="text-xs text-muted-foreground">
                                    {summaryTasks.conversation_running
                                        ? `后台运行中：${summaryTasks.conversation_running_count || 1} 个总结任务`
                                        : lightLoading
                                            ? "正在检查轻量模型配置…"
                                            : lightOptions.some(o => o.value === (conversationSummaryModel || defaultLightModel))
                                                ? "空闲，保存不会阻塞后台总结任务。"
                                                : "未配置轻量模型，已暂停"}
                                </span>
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    disabled={
                                        saving ||
                                        runningAction.conversation ||
                                        summaryTasks.conversation_running ||
                                        lightLoading ||
                                        !lightOptions.some(o => o.value === (conversationSummaryModel || defaultLightModel))
                                    }
                                    onClick={() => void handleTriggerTask("conversation")}
                                >
                                    {summaryTasks.conversation_running ? "总结进行中…" : "立即触发对话总结"}
                                </Button>
                            </div>
                        </div>
                    ) : null}
                </SettingsGroup>

                {/* 7. 表单自动填写 */}
                <SettingsGroup
                    id="form-autofill"
                    title="表单自动填写"
                    description="根据对话上下文自动协助填写参数或表单字段。"
                >
                    <div className={settingRowClassName}>
                        <SettingRowText label="启用表单自动填写" />
                        <Switch
                            aria-label="启用表单自动填写"
                            checked={formAutofillEnabled}
                            onCheckedChange={setFormAutofillEnabled}
                        />
                    </div>
                    {formAutofillEnabled ? (
                        <div className={settingRowClassName}>
                            <SettingRowText label="表单填写模型" />
                            <div className="w-64 shrink-0">
                                <Select
                                    disabled={lightLoading}
                                    value={formAutofillModel || INHERIT_VALUE}
                                    onValueChange={(v) => setFormAutofillModel(v === INHERIT_VALUE ? "" : v)}
                                >
                                    <SelectTrigger className="w-full" aria-label="表单填写模型">
                                        <SelectValue />
                                    </SelectTrigger>
                                    <SelectContent>
                                        <SelectItem value={INHERIT_VALUE}>
                                            {lightModelLabel
                                                ? `沿用默认轻量模型（${lightModelLabel}）`
                                                : "沿用默认轻量模型"}
                                        </SelectItem>
                                        {lightOptions.map((o) => (
                                            <SelectItem key={o.value} value={o.value}>
                                                {o.label}
                                            </SelectItem>
                                        ))}
                                    </SelectContent>
                                </Select>
                            </div>
                        </div>
                    ) : null}
                </SettingsGroup>

                <ConfigSaveFooter>
                    <div className="flex items-center justify-between gap-3">
                        <div className="flex items-center gap-2">
                            <Button
                                type="button"
                                disabled={saving || !isDirty}
                                onClick={() => void handleSaveAll()}
                            >
                                {saving ? "保存中…" : "保存配置"}
                            </Button>
                            {isDirty ? (
                                <Button
                                    type="button"
                                    variant="outline"
                                    disabled={saving}
                                    onClick={hydrateFromConfig}
                                >
                                    放弃更改
                                </Button>
                            ) : null}
                        </div>
                        {isDirty ? (
                            <span className="text-xs text-muted-foreground">有未保存的更改</span>
                        ) : null}
                    </div>
                </ConfigSaveFooter>
            </SettingsPageShell>
            {leaveDialog}
        </>
    );
};

export default AiAutomationConfig;
