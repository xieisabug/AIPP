import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { AlertTriangle, Check, ChevronDown, ChevronLeft, Eye, Loader2, Mic, Plus, RefreshCw, Search, Video, X } from "lucide-react";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogTitle } from "../ui/dialog";
import { Button } from "../ui/button";
import { Input } from "../ui/input";
import { PasswordInput } from "../ui/password-input";
import { ProviderProxySwitch, useProviderProxyConfigured } from "./ProviderProxySwitch";
import { Checkbox } from "../ui/checkbox";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "../ui/collapsible";
import VaultRefField from "./vault/VaultRefField";
import { useVaultItems } from "@/hooks/useVaultItems";
import { getProviderEndpointStatus } from "@/utils/providerEndpoint";
import { PinyinFilter, usePinyinReady } from "@/utils/pinyinFilter";
import { cn } from "@/utils/utils";
import type { ModelForSelection, ModelSelectionResponse } from "./llmModelTypes";
import {
    LOCAL_AGENT_DEFAULTS_READABLE,
    newProviderNameError,
    suggestProviderName,
    type LocalAgentDefaults,
    type ProviderApiTypeOption,
} from "./providerApiTypes";
import { useLocalAgentDetection, type LocalAgentStatus } from "./useLocalAgentDetection";
import LocalAgentSetup, { type DefaultsState } from "./LocalAgentSetup";
import type { MCPServer } from "@/data/MCP";
import { connectDeepSeekSearch, findSearchServer, hideKey, searchServiceLabel } from "./deepseekSuite";
import { parseVaultValue } from "./vault/vaultRefs";
import { describeBalance, useDeepSeekBalance, type BalanceState } from "./useDeepSeekBalance";

export interface NewProviderWizardProps {
    open: boolean;
    onClose: () => void;
    /** 当前平台可新建的类型。 */
    apiTypes: readonly ProviderApiTypeOption[];
    /** 已有提供商：名称查重（向导自己创建的草稿会被排除）与「已添加」标识。 */
    existingProviders: ReadonlyArray<{ id: string | number; name: string; api_type?: string }>;
    /** 完成向导：提供商已写入连接信息与模型并启用。 */
    onCreated: (providerId: number) => void;
}

type Step = 1 | 2 | 3;
type FetchState = { status: "idle" | "loading" | "done" } | { status: "error"; message: string };

/**
 * 新增模型提供商向导：① 选择 API 类型 → ② 名称 / 接口地址 / API Key → ③ 获取并勾选模型。
 * DeepSeek 在第三步可一并把内置搜索（Search / Fetch）接到同一把 Key 上（需求 335）。
 *
 * 第二步「下一步」时创建一个未启用的草稿提供商并写入连接信息，第三步复用已有的
 * `preview_model_list`（保险箱引用、代理、各类型路由都沿用设置页链路）；完成时写入模型并启用。
 * 未完成就关闭（取消、Esc、离开设置页）会删除草稿。
 */
const NewProviderWizard: React.FC<NewProviderWizardProps> = ({ open, onClose, ...rest }) => (
    <Dialog open={open} onOpenChange={(next) => !next && onClose()}>
        <DialogContent
            showCloseButton={false}
            // 向导有多步输入，点到外面不关闭；用关闭按钮或 Esc 退出
            onInteractOutside={(event) => event.preventDefault()}
            className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl !max-h-[min(88dvh,var(--aipp-vv-height,88dvh))] top-[max(1rem,var(--aipp-vv-offset-top,0px))] translate-y-0 data-[state=open]:slide-in-from-top-2 sm:top-[50%] sm:translate-y-[-50%]"
            data-aipp-slot="new-provider-wizard"
        >
            <WizardContent onClose={onClose} {...rest} />
        </DialogContent>
    </Dialog>
);

const LOCAL_DEFAULT_VALUES: Readonly<Record<string, string>> = {
    codex_approval_policy: "on-request",
    codex_sandbox: "workspace-write",
    acp_cli_command: "",
};

/** 各本地智能体写入提供商的设置键。 */
const LOCAL_SETTING_KEYS: Record<string, readonly string[]> = {
    codex_app_server: ["codex_approval_policy", "codex_sandbox"],
    acp: ["acp_cli_command"],
};

const isEnterSubmit = (event: React.KeyboardEvent) =>
    event.key === "Enter" && !event.nativeEvent.isComposing && event.keyCode !== 229;

/** 未选择的模型默认使用第一个可用协议（与选择模型弹窗一致）。 */
const normalizeModels = (models: ModelForSelection[], keepSelected: ReadonlySet<string>) =>
    models.map((model) => {
        const base = !model.is_selected && model.available_modes?.length
            ? { ...model, request_mode: model.available_modes[0] }
            : model;
        return keepSelected.has(model.code) && base.request_mode !== "unsupported" ? { ...base, is_selected: true } : base;
    });

const WizardContent: React.FC<Omit<NewProviderWizardProps, "open">> = ({ onClose, apiTypes, existingProviders, onCreated }) => {
    const [step, setStep] = useState<Step>(1);
    const [apiType, setApiType] = useState<ProviderApiTypeOption | null>(null);
    const [name, setName] = useState("");
    const [nameError, setNameError] = useState<string | null>(null);
    const [endpoint, setEndpoint] = useState("");
    const [apiKey, setApiKey] = useState("");
    const [proxyEnabled, setProxyEnabled] = useState(false);
    const proxyConfigured = useProviderProxyConfigured();
    const [advancedOpen, setAdvancedOpen] = useState(false);
    const [busy, setBusy] = useState(false);
    const [fetchState, setFetchState] = useState<FetchState>({ status: "idle" });
    const [models, setModels] = useState<ModelForSelection[]>([]);
    const [customCodes, setCustomCodes] = useState<string[]>([]);
    const [customInput, setCustomInput] = useState("");
    const [search, setSearch] = useState("");
    const pinyinReady = usePinyinReady(search.trim().length > 0);
    const vault = useVaultItems();
    const detection = useLocalAgentDetection(apiTypes);
    // DeepSeek 全家桶：一并接入内置搜索
    const [suiteSearch, setSuiteSearch] = useState(true);
    const [searchServers, setSearchServers] = useState<MCPServer[] | null>(null);
    const [suiteError, setSuiteError] = useState<string | null>(null);
    // 本地智能体写入新提供商的设置；初值与详情页表单默认值一致，读到智能体自身配置后覆盖
    const [localValues, setLocalValues] = useState<Record<string, string>>(LOCAL_DEFAULT_VALUES);
    const [defaultsByType, setDefaultsByType] = useState<Record<string, DefaultsState>>({});
    // 用户手动改过的设置，读取结果晚到时不覆盖
    const touchedLocalRef = useRef(new Set<string>());

    // 已创建的草稿；卸载时（任何方式关闭）若未完成则删除
    const draftRef = useRef<{ id: number; apiType: string } | null>(null);
    const finishedRef = useRef(false);
    const fetchSeqRef = useRef(0);
    // 自动预填的值：用户没改过时，切换类型会跟着替换
    const autoFillRef = useRef({ name: "", endpoint: "" });
    // 已成功写入的自定义模型，完成失败后重试不重复添加
    const addedCustomRef = useRef(new Set<string>());

    useEffect(() => () => {
        const draft = draftRef.current;
        if (draft && !finishedRef.current) {
            invoke("delete_llm_provider", { llmProviderId: draft.id }).catch((error) => {
                toast.error(`删除未完成的提供商草稿失败（provider=${draft.id}）: ${String(error)}`);
            });
        }
    }, []);

    const isHttp = apiType?.kind === "http";
    const totalSteps = apiType && !isHttp ? 2 : 3;
    const stepLabel = step === 1
        ? "选择 API 类型"
        : step === 2
            ? (isHttp ? "填写连接信息" : apiType?.kind === "local" ? "确认本机配置" : "命名提供商")
            : "选择模型";
    const endpointStatus = useMemo(() => getProviderEndpointStatus(endpoint), [endpoint]);
    const vaultTarget = useMemo(() => {
        try {
            return endpoint.trim() ? new URL(endpoint.trim()).origin : "该 Provider 的默认接口地址";
        } catch {
            return null;
        }
    }, [endpoint]);

    const otherNames = useMemo(() => {
        const draftId = draftRef.current?.id;
        return existingProviders
            .filter((provider) => draftId === undefined || String(provider.id) !== String(draftId))
            .map((provider) => provider.name);
        // draftRef 变化总伴随 existingProviders 刷新（llm_provider_changed）
    }, [existingProviders]);

    const deleteDraft = useCallback(async () => {
        const draft = draftRef.current;
        if (!draft) return;
        await invoke("delete_llm_provider", { llmProviderId: draft.id });
        draftRef.current = null;
    }, []);

    const chooseType = useCallback(async (type: ProviderApiTypeOption) => {
        if (busy) return;
        if (draftRef.current && draftRef.current.apiType !== type.value) {
            // 草稿的类型不能改：换类型时删掉旧草稿，下一步重新创建
            setBusy(true);
            try {
                await deleteDraft();
            } catch (error) {
                toast.error(`更换类型失败：删除之前创建的提供商草稿出错: ${String(error)}`);
                return;
            } finally {
                setBusy(false);
            }
            setModels([]);
            setCustomCodes([]);
            setFetchState({ status: "idle" });
            addedCustomRef.current.clear();
        }
        const auto = autoFillRef.current;
        if (!name.trim() || name === auto.name) {
            const suggested = suggestProviderName(type.label, otherNames);
            setName(suggested);
            auto.name = suggested;
        }
        if (!endpoint.trim() || endpoint === auto.endpoint) {
            const suggested = type.defaultEndpoint ?? "";
            setEndpoint(suggested);
            auto.endpoint = suggested;
        }
        setNameError(null);
        setApiType(type);
        setStep(2);
        if (LOCAL_AGENT_DEFAULTS_READABLE.has(type.value) && !defaultsByType[type.value]) {
            setDefaultsByType((prev) => ({ ...prev, [type.value]: { status: "loading" } }));
            invoke<LocalAgentDefaults>("read_local_agent_defaults", { apiType: type.value })
                .then((defaults) => {
                    setDefaultsByType((prev) => ({ ...prev, [type.value]: { status: "done", defaults } }));
                    setLocalValues((prev) => {
                        const next = { ...prev };
                        for (const [key, value] of Object.entries(defaults.settings)) {
                            if (!touchedLocalRef.current.has(key)) next[key] = value;
                        }
                        return next;
                    });
                })
                .catch((error) => {
                    setDefaultsByType((prev) => ({ ...prev, [type.value]: { status: "error", message: String(error) } }));
                });
        }
    }, [busy, deleteDraft, name, endpoint, otherNames, defaultsByType]);

    // ACP 默认选中第一个已安装的 CLI
    useEffect(() => {
        const installed = detection.acpClis?.find((cli) => cli.installed);
        if (!installed || touchedLocalRef.current.has("acp_cli_command")) return;
        setLocalValues((prev) => (prev.acp_cli_command ? prev : { ...prev, acp_cli_command: installed.value }));
    }, [detection.acpClis]);

    const changeLocalValue = useCallback((key: string, value: string) => {
        touchedLocalRef.current.add(key);
        setLocalValues((prev) => ({ ...prev, [key]: value }));
    }, []);

    const loadModels = useCallback(async (providerId: number, keepSelected: ReadonlySet<string>) => {
        const seq = ++fetchSeqRef.current;
        setFetchState({ status: "loading" });
        try {
            const data = await invoke<ModelSelectionResponse>("preview_model_list", { llmProviderId: providerId });
            if (seq !== fetchSeqRef.current) return;
            setModels(normalizeModels(data.available_models, keepSelected));
            setFetchState({ status: "done" });
        } catch (error) {
            if (seq !== fetchSeqRef.current) return;
            setFetchState({ status: "error", message: String(error) });
        }
    }, []);

    /** 创建或更新草稿并写入连接信息，返回提供商 ID。 */
    const saveConnection = useCallback(async (type: ProviderApiTypeOption, trimmedName: string): Promise<number> => {
        let id = draftRef.current?.id;
        if (id === undefined) {
            id = await invoke<number>("add_llm_provider", { name: trimmedName, apiType: type.value });
            draftRef.current = { id, apiType: type.value };
        } else {
            await invoke("update_llm_provider", { id, name: trimmedName, apiType: type.value, description: "", isEnabled: false });
        }
        const write = (key: string, value: string) =>
            invoke("update_llm_provider_config", { llmProviderId: id, name: key, value });
        // 接口地址先于 API Key 写入：保险箱引用按当前地址确认去向
        if (type.kind === "http" && !type.noEndpoint) await write("endpoint", endpoint.trim());
        await write("proxy_enabled", proxyConfigured && proxyEnabled ? "true" : "false");
        if (type.kind === "http") await write("api_key", apiKey.trim());
        for (const key of LOCAL_SETTING_KEYS[type.value] ?? []) {
            const value = localValues[key]?.trim();
            if (value) await write(key, value);
        }
        return id;
    }, [endpoint, apiKey, proxyConfigured, proxyEnabled, localValues]);

    const enableProvider = useCallback(async (id: number, type: ProviderApiTypeOption, trimmedName: string, close = true) => {
        await invoke("update_llm_provider", { id, name: trimmedName, apiType: type.value, description: "", isEnabled: true });
        finishedRef.current = true;
        toast.success("添加大模型提供商成功");
        onCreated(id);
        if (close) onClose();
    }, [onCreated, onClose]);

    const isDeepSeek = apiType?.value === "deepseek";
    // 第三步时草稿与 Key 已保存：查余额同时验证 Key 是否可用
    const balance = useDeepSeekBalance(step === 3 && isDeepSeek && apiKey.trim() ? draftRef.current?.id ?? null : null, true);
    useEffect(() => {
        if (step !== 3 || !isDeepSeek || searchServers) return;
        // 仅用于提示「当前使用 X」；读不到时不提示，不影响向导
        invoke<MCPServer[]>("get_mcp_servers")
            .then((list) => setSearchServers(Array.isArray(list) ? list : []))
            .catch(() => setSearchServers([]));
    }, [step, isDeepSeek, searchServers]);

    /** 能力单独执行、单独报告：失败不回滚已启用的服务商，可重试或跳过。 */
    const connectSuite = useCallback(async (providerId: number) => {
        setSuiteError(null);
        try {
            await connectDeepSeekSearch({ providerId, providerName: name.trim(), apiKey, endpoint, vault });
            toast.success("内置搜索已改用 DeepSeek");
            onClose();
        } catch (error) {
            setSuiteError(hideKey(String(error), apiKey));
        }
    }, [name, apiKey, endpoint, vault, onClose]);

    const submitConnection = useCallback(async () => {
        if (!apiType || busy) return;
        const error = newProviderNameError(name, otherNames);
        if (error) {
            setNameError(error);
            return;
        }
        if (isHttp && !apiType.noEndpoint && endpointStatus.kind === "invalid") return;
        const trimmedName = name.trim();
        setBusy(true);
        try {
            const id = await saveConnection(apiType, trimmedName);
            if (!isHttp) {
                await enableProvider(id, apiType, trimmedName);
                return;
            }
            setStep(3);
            const keep = new Set(models.filter((model) => model.is_selected).map((model) => model.code));
            void loadModels(id, keep);
        } catch (e) {
            const detail = String(e);
            const safe = apiKey.trim() ? detail.split(apiKey.trim()).join("[已隐藏]") : detail;
            toast.error(`保存提供商失败: ${safe}`);
        } finally {
            setBusy(false);
        }
    }, [apiType, busy, name, otherNames, isHttp, endpointStatus.kind, saveConnection, enableProvider, models, loadModels, apiKey]);

    const selectedCount = models.filter((model) => model.is_selected).length + customCodes.length;
    const finishDisabled = busy || fetchState.status === "loading" || (!finishedRef.current && selectedCount === 0);

    const finish = useCallback(async () => {
        const draft = draftRef.current;
        if (!apiType || !draft || finishDisabled) return;
        const withSearch = apiType.value === "deepseek" && suiteSearch && !!apiKey.trim();
        setBusy(true);
        try {
            // 服务商已启用（能力接入失败后重试）时只重试能力
            if (!finishedRef.current) {
                if (models.some((model) => model.is_selected)) {
                    await invoke("update_selected_models", { llmProviderId: draft.id, selectedModels: models });
                }
                for (const code of customCodes) {
                    if (addedCustomRef.current.has(code)) continue;
                    await invoke("add_llm_model", { code, llmProviderId: draft.id });
                    addedCustomRef.current.add(code);
                }
                await enableProvider(draft.id, apiType, name.trim(), !withSearch);
            }
            if (withSearch) await connectSuite(draft.id);
        } catch (error) {
            toast.error(`保存模型失败: ${String(error)}`);
        } finally {
            setBusy(false);
        }
    }, [apiType, finishDisabled, models, customCodes, enableProvider, name, suiteSearch, apiKey, connectSuite]);

    const addCustomModel = useCallback(() => {
        const code = customInput.trim();
        if (!code) return;
        const fetched = models.find((model) => model.code === code);
        if (fetched) {
            if (fetched.request_mode === "unsupported") {
                toast.error(`模型 ${code} 的协议暂不支持`);
                return;
            }
            setModels((prev) => prev.map((model) => model.code === code ? { ...model, is_selected: true } : model));
        } else if (!customCodes.includes(code)) {
            setCustomCodes((prev) => [...prev, code]);
        }
        setCustomInput("");
    }, [customInput, models, customCodes]);

    const toggleModel = useCallback((code: string, checked: boolean) => {
        setModels((prev) => prev.map((model) =>
            model.code === code && (!checked || model.request_mode !== "unsupported") ? { ...model, is_selected: checked } : model));
    }, []);

    const filteredModels = useMemo(() => {
        const query = search.trim();
        if (!query) return models;
        return models.filter((model) => PinyinFilter.matches(model.name, query) || PinyinFilter.matches(model.code, query));
        // pinyinReady：拼音库加载完成后重新过滤
        // eslint-disable-next-line react-hooks/exhaustive-deps
    }, [models, search, pinyinReady]);

    const addedCounts = useMemo(() => {
        const draftId = draftRef.current?.id;
        const counts = new Map<string, number>();
        for (const provider of existingProviders) {
            if (!provider.api_type || (draftId !== undefined && String(provider.id) === String(draftId))) continue;
            counts.set(provider.api_type, (counts.get(provider.api_type) ?? 0) + 1);
        }
        return counts;
    }, [existingProviders]);

    const apiGroups = useMemo(() => [
        { title: "模型 API", local: false, items: apiTypes.filter((type) => type.kind !== "local") },
        { title: "本地智能体", local: true, items: apiTypes.filter((type) => type.kind === "local") },
    ].filter((group) => group.items.length > 0), [apiTypes]);

    return (
        <>
            {/* 紧凑标题栏：标题 + 当前步骤，底边是进度条 */}
            <div className="relative flex shrink-0 items-center gap-3 border-b border-border px-4 py-2.5 sm:px-5">
                <DialogTitle className="truncate text-base font-semibold">新增模型提供商</DialogTitle>
                <span className="truncate text-xs text-muted-foreground" data-testid="wizard-step">
                    {step}/{totalSteps} · {stepLabel}
                </span>
                <DialogDescription className="sr-only">选择 API 类型，填写连接信息并选择模型</DialogDescription>
                <DialogClose asChild>
                    <button
                        type="button"
                        aria-label="关闭"
                        className="ml-auto flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
                    >
                        <X className="h-4 w-4" />
                    </button>
                </DialogClose>
                <div
                    aria-hidden
                    className="absolute bottom-[-1px] left-0 h-0.5 bg-foreground/70 transition-[width] duration-300"
                    style={{ width: `${(step / totalSteps) * 100}%` }}
                />
            </div>

            <div className={cn("min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 py-4 sm:px-5", step === 3 && "flex min-h-[min(60dvh,26rem)] flex-col")}>
                {step === 1 && (
                    <div className="space-y-4">
                        {apiGroups.map((group) => (
                            <section key={group.title} className="space-y-2">
                                <h3 className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                                    {group.title}
                                    {group.local && Object.values(detection.statuses).some((status) => status.state === "detecting") && (
                                        <span className="flex items-center gap-1 font-normal"><Loader2 className="h-3 w-3 animate-spin" />检测本机安装…</span>
                                    )}
                                </h3>
                                <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
                                    {group.items.map((type) => (
                                        <button
                                            key={type.value}
                                            type="button"
                                            disabled={busy}
                                            aria-pressed={apiType?.value === type.value}
                                            onClick={() => void chooseType(type)}
                                            className="flex min-w-0 flex-col items-start gap-0.5 rounded-lg border border-border px-3 py-2.5 text-left transition-colors hover:border-foreground/30 hover:bg-muted/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:opacity-60 aria-[pressed=true]:border-foreground/60 aria-[pressed=true]:bg-muted/60"
                                        >
                                            <span className="w-full truncate text-sm font-medium text-foreground">{type.label}</span>
                                            <span className="w-full truncate text-xs text-muted-foreground">{type.description}</span>
                                            <TypeBadges installed={detection.statuses[type.value]} added={addedCounts.get(type.value) ?? 0} />
                                        </button>
                                    ))}
                                </div>
                            </section>
                        ))}
                    </div>
                )}

                {step === 2 && apiType && (
                    <div className="space-y-4">
                        <div className="flex items-center justify-between gap-3 rounded-lg bg-muted/60 px-3 py-2">
                            <div className="min-w-0">
                                <div className="truncate text-sm font-medium">{apiType.label}</div>
                                <div className="truncate text-xs text-muted-foreground">{apiType.description}</div>
                            </div>
                            <Button type="button" variant="ghost" size="sm" className="h-7 shrink-0 px-2 text-xs" disabled={busy} onClick={() => setStep(1)}>
                                更换
                            </Button>
                        </div>

                        <div className="space-y-1.5">
                            <label htmlFor="new-provider-name" className="text-sm font-medium text-foreground">提供商名称</label>
                            <Input autoComplete="off"
                                id="new-provider-name"
                                autoFocus
                                autoCapitalize="none"
                                enterKeyHint={isHttp ? "next" : "done"}
                                placeholder="例如：我的 OpenAI"
                                value={name}
                                aria-invalid={nameError ? true : undefined}
                                aria-describedby={nameError ? "new-provider-name-error" : undefined}
                                onChange={(event) => {
                                    setName(event.target.value);
                                    if (nameError) setNameError(null);
                                }}
                                onKeyDown={(event) => {
                                    if (!isEnterSubmit(event)) return;
                                    event.preventDefault();
                                    void submitConnection();
                                }}
                            />
                            {nameError && (
                                <p id="new-provider-name-error" role="alert" className="text-xs text-destructive">{nameError}</p>
                            )}
                        </div>

                        {isHttp && !apiType.noEndpoint && (
                            <div className="space-y-1.5">
                                <label htmlFor="new-provider-endpoint" className="text-sm font-medium text-foreground">接口地址</label>
                                <Input autoComplete="off"
                                    id="new-provider-endpoint"
                                    inputMode="url"
                                    autoCapitalize="none"
                                    spellCheck={false}
                                    placeholder={apiType.value === "magpie" ? "留空连接本机 magpie" : apiType.defaultEndpoint}
                                    value={endpoint}
                                    aria-invalid={endpointStatus.kind === "invalid" ? true : undefined}
                                    onChange={(event) => setEndpoint(event.target.value)}
                                />
                                {endpointStatus.kind === "invalid" ? (
                                    <p role="alert" className="text-xs text-destructive">{endpointStatus.message}</p>
                                ) : apiType.value === "magpie" && endpointStatus.kind === "unset" ? (
                                    <p className="text-xs text-muted-foreground">留空则连接本机 magpie（默认 127.0.0.1:3425，端口以 settings.json 为准）。</p>
                                ) : apiType.defaultEndpoint ? (
                                    <p className="text-xs text-muted-foreground">已填入默认地址，使用代理或中转服务时可修改。</p>
                                ) : null}
                            </div>
                        )}

                        {isHttp && (
                            <div className="space-y-1.5">
                                <label htmlFor="new-provider-api-key" className="text-sm font-medium text-foreground">API Key</label>
                                {apiType.value === "magpie" ? (
                                    <PasswordInput
                                        id="new-provider-api-key"
                                        autoComplete="off"
                                        value={apiKey}
                                        onChange={(event) => setApiKey(event.target.value)}
                                    />
                                ) : (
                                    <VaultRefField
                                        vault={vault}
                                        id="new-provider-api-key"
                                        ariaLabel="API Key"
                                        placeholder={apiType.value === "ollama" ? "本机 Ollama 通常无需填写" : undefined}
                                        value={apiKey}
                                        onChange={setApiKey}
                                        suggestedName={`${name.trim() || apiType.label} · API Key`}
                                        targetHint={vaultTarget}
                                    />
                                )}
                                {apiType.value === "magpie" ? (
                                    <p className="text-xs text-muted-foreground">留空时使用 magpie-aipp。访问另一台电脑上的网关时，填写那里的网关密钥。</p>
                                ) : apiType.value === "opencode_go" ? (
                                    <a className="text-xs text-muted-foreground underline underline-offset-2" href="https://opencode.ai/auth" target="_blank" rel="noreferrer">
                                        从 OpenCode Console 获取订阅 API Key
                                    </a>
                                ) : null}
                            </div>
                        )}

                        {apiType.kind === "copilot" && (
                            <p className="text-xs text-muted-foreground">创建后在提供商详情页完成 GitHub 授权并获取模型。</p>
                        )}

                        {apiType.kind === "local" && (
                            <LocalAgentSetup
                                apiType={apiType}
                                status={detection.statuses[apiType.value]}
                                acpClis={detection.acpClis}
                                defaults={defaultsByType[apiType.value]}
                                values={localValues}
                                onChange={changeLocalValue}
                            />
                        )}

                        <Collapsible open={advancedOpen} onOpenChange={setAdvancedOpen}>
                            <CollapsibleTrigger asChild>
                                <button type="button" className="flex items-center gap-1 text-xs font-medium text-muted-foreground hover:text-foreground">
                                    高级配置
                                    <ChevronDown className={cn("h-3.5 w-3.5 transition-transform", advancedOpen && "rotate-180")} />
                                </button>
                            </CollapsibleTrigger>
                            <CollapsibleContent className="mt-2">
                                <div className="rounded-lg border border-border px-3 py-2.5">
                                    <ProviderProxySwitch
                                        id="new-provider-proxy"
                                        description={apiType.kind === "local" ? "使用全局网络代理启动此 Agent" : "使用全局网络代理配置请求模型与获取模型列表"}
                                        configured={proxyConfigured}
                                        checked={proxyEnabled}
                                        onCheckedChange={setProxyEnabled}
                                        busy={busy}
                                        onLeaveWizard={onClose}
                                    />
                                </div>
                            </CollapsibleContent>
                        </Collapsible>
                    </div>
                )}

                {step === 3 && (
                    <div className="flex min-h-0 flex-1 flex-col gap-3">
                        <div className="flex gap-2">
                            <Input autoComplete="off"
                                aria-label="自定义模型"
                                placeholder="输入模型 ID 添加自定义模型"
                                enterKeyHint="done"
                                value={customInput}
                                onChange={(event) => setCustomInput(event.target.value)}
                                onKeyDown={(event) => {
                                    if (!isEnterSubmit(event)) return;
                                    event.preventDefault();
                                    addCustomModel();
                                }}
                            />
                            <Button type="button" variant="outline" className="shrink-0 gap-1" disabled={!customInput.trim()} onClick={addCustomModel}>
                                <Plus className="h-4 w-4" />
                                添加
                            </Button>
                        </div>

                        {customCodes.length > 0 && (
                            <div className="flex flex-wrap gap-1.5">
                                {customCodes.map((code) => (
                                    <span key={code} className="inline-flex max-w-full items-center gap-1 rounded-md border border-border bg-muted/60 py-0.5 pl-2 pr-1 text-xs">
                                        <span className="truncate">{code}</span>
                                        <button
                                            type="button"
                                            aria-label={`移除 ${code}`}
                                            className="rounded p-0.5 text-muted-foreground hover:bg-background hover:text-foreground"
                                            onClick={() => setCustomCodes((prev) => prev.filter((item) => item !== code))}
                                        >
                                            <X className="h-3 w-3" />
                                        </button>
                                    </span>
                                ))}
                            </div>
                        )}

                        {fetchState.status === "loading" && (
                            <div className="flex flex-1 items-center justify-center gap-2 py-10 text-sm text-muted-foreground">
                                <Loader2 className="h-4 w-4 animate-spin" />
                                正在获取模型列表…
                            </div>
                        )}

                        {fetchState.status === "error" && (
                            <div role="alert" className="space-y-2 rounded-lg border border-border bg-muted/40 p-3">
                                <div className="flex items-center gap-2 text-sm font-medium">
                                    <AlertTriangle className="h-4 w-4 text-muted-foreground" />
                                    获取模型列表失败
                                </div>
                                <p className="break-all text-xs text-muted-foreground">{fetchState.message}</p>
                                <p className="text-xs text-muted-foreground">可返回上一步检查接口地址、API Key 或代理设置，也可以直接添加自定义模型。</p>
                                <Button
                                    type="button"
                                    variant="outline"
                                    size="sm"
                                    className="gap-1"
                                    onClick={() => draftRef.current && void loadModels(draftRef.current.id, new Set(models.filter((m) => m.is_selected).map((m) => m.code)))}
                                >
                                    <RefreshCw className="h-3.5 w-3.5" />
                                    重试
                                </Button>
                            </div>
                        )}

                        {fetchState.status === "done" && (
                            models.length === 0 ? (
                                <p className="py-8 text-center text-sm text-muted-foreground">接口没有返回模型，可在上方添加自定义模型。</p>
                            ) : (
                                <>
                                    <div className="flex items-center gap-2">
                                        <div className="relative flex-1">
                                            <Search className="absolute left-2.5 top-1/2 h-3.5 w-3.5 -translate-y-1/2 text-muted-foreground" />
                                            <Input autoComplete="off" value={search} onChange={(event) => setSearch(event.target.value)} placeholder="搜索模型..." className="h-8 pl-8 text-sm" />
                                        </div>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            className="h-8 px-2 text-xs"
                                            onClick={() => setModels((prev) => prev.map((model) => ({ ...model, is_selected: model.request_mode !== "unsupported" })))}
                                        >
                                            全选
                                        </Button>
                                        <Button
                                            type="button"
                                            variant="ghost"
                                            size="sm"
                                            className="h-8 px-2 text-xs"
                                            onClick={() => setModels((prev) => prev.map((model) => ({ ...model, is_selected: false })))}
                                        >
                                            清空
                                        </Button>
                                    </div>
                                    <div className="grid min-h-0 grid-cols-1 content-start gap-1.5 overflow-y-auto sm:grid-cols-2" data-testid="wizard-model-list">
                                        {filteredModels.map((model) => (
                                            <label
                                                key={model.code}
                                                className={cn(
                                                    "flex min-w-0 cursor-pointer items-center gap-2.5 rounded-md border border-border px-2.5 py-2 transition-colors hover:bg-muted/60",
                                                    model.is_selected && "border-foreground/40 bg-muted/60",
                                                )}
                                            >
                                                <Checkbox
                                                    aria-label={model.name}
                                                    checked={model.is_selected}
                                                    disabled={model.request_mode === "unsupported" && !model.is_selected}
                                                    onCheckedChange={(checked) => toggleModel(model.code, checked === true)}
                                                />
                                                <span className="min-w-0 flex-1 truncate text-sm" title={model.name === model.code ? model.code : `${model.name}\n${model.code}`}>
                                                    {model.name}
                                                </span>
                                                <span className="flex shrink-0 gap-1 text-muted-foreground">
                                                    {model.vision_support && <Eye className="h-3 w-3" />}
                                                    {model.audio_support && <Mic className="h-3 w-3" />}
                                                    {model.video_support && <Video className="h-3 w-3" />}
                                                </span>
                                            </label>
                                        ))}
                                    </div>
                                </>
                            )
                        )}

                        {isDeepSeek && (
                            <DeepSeekSuiteOptions
                                checked={suiteSearch}
                                onCheckedChange={setSuiteSearch}
                                disabled={busy || finishedRef.current}
                                hasKey={!!apiKey.trim()}
                                keyIsRef={!!parseVaultValue(apiKey.trim())}
                                vaultAvailable={vault.available}
                                currentSearch={searchServers ? searchServiceLabel(findSearchServer(searchServers)) : null}
                                error={suiteError}
                                onSkip={onClose}
                                balance={apiKey.trim() ? balance.state : null}
                            />
                        )}
                    </div>
                )}
            </div>

            {step > 1 && (
                // 底部内边距 = 顶部 + 安全区；不用 .safe-bottom（未分层 CSS 会把 pb 覆盖成 0）
                <div className="flex shrink-0 items-center gap-2 border-t border-border px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                    <Button type="button" variant="ghost" className="min-h-11 gap-1 px-3 sm:min-h-9" disabled={busy || finishedRef.current} onClick={() => setStep(step === 3 ? 2 : 1)}>
                        <ChevronLeft className="h-4 w-4" />
                        上一步
                    </Button>
                    <div className="ml-auto flex items-center gap-3">
                        {step === 3 && !finishedRef.current && (
                            <span role="status" className="text-xs text-muted-foreground">
                                {selectedCount === 0 ? "请至少选择一个模型" : `已选 ${selectedCount} 个模型`}
                            </span>
                        )}
                        {step === 2 ? (
                            <Button type="button" className="min-h-11 px-5 sm:min-h-9" disabled={busy} onClick={() => void submitConnection()}>
                                {busy && <Loader2 className="h-4 w-4 animate-spin" />}
                                {isHttp ? "下一步" : "完成"}
                            </Button>
                        ) : (
                            <Button type="button" className="min-h-11 px-5 sm:min-h-9" disabled={finishDisabled} onClick={() => void finish()}>
                                {busy && <Loader2 className="h-4 w-4 animate-spin" />}
                                {suiteError ? "重试" : "完成"}
                            </Button>
                        )}
                    </div>
                </div>
            )}
        </>
    );
};

export default NewProviderWizard;

/** 第三步底部「一并启用」：目前只有搜索工具；对话内联网、语音等后续能力加在这里。 */
const DeepSeekSuiteOptions: React.FC<{
    checked: boolean;
    onCheckedChange: (checked: boolean) => void;
    disabled: boolean;
    hasKey: boolean;
    keyIsRef: boolean;
    vaultAvailable: boolean;
    currentSearch: string | null;
    error: string | null;
    onSkip: () => void;
    /** 账户余额（同时说明 Key 是否可用）；没有 Key 时为 null。 */
    balance: BalanceState | null;
}> = ({ checked, onCheckedChange, disabled, hasKey, keyIsRef, vaultAvailable, currentSearch, error, onSkip, balance }) => {
    const notes: string[] = ["助手的 search_http 与 fetch_url 改用 DeepSeek Search / Fetch，按成功次数计费（每千次 5 元）。"];
    if (!hasKey) notes.push("需要先在上一步填写 API Key。");
    else if (!keyIsRef) notes.push(vaultAvailable ? "API Key 将存入凭据保险箱，模型与搜索共用同一条目。" : "此设备不支持凭据保险箱，搜索设置中会保存一份 Key。");
    if (currentSearch && currentSearch !== "DeepSeek") notes.push("当前使用 " + currentSearch + "，将改为 DeepSeek。");
    return (
        <section className="shrink-0 space-y-1.5 border-t border-border pt-3" data-aipp-slot="deepseek-suite-options">
            <div className="flex items-baseline justify-between gap-3">
                <h3 className="text-xs font-medium text-muted-foreground">一并启用</h3>
                {balance && balance.status !== "idle" && (
                    <span className="flex min-w-0 items-center gap-1 truncate text-xs text-muted-foreground" data-testid="wizard-deepseek-balance">
                        {balance.status === "done" && balance.balance.available && <Check className="h-3 w-3 shrink-0" />}
                        <span className="truncate">账户余额 {describeBalance(balance)}</span>
                    </span>
                )}
            </div>
            <label className={cn("flex items-start gap-2.5 rounded-md border border-border px-2.5 py-2", checked && hasKey && "border-foreground/40 bg-muted/60")}>
                <Checkbox
                    aria-label="同时用于搜索工具"
                    className="mt-0.5"
                    checked={checked && hasKey}
                    disabled={disabled || !hasKey}
                    onCheckedChange={(value) => onCheckedChange(value === true)}
                />
                <span className="min-w-0 flex-1">
                    <span className="block text-sm">同时用于搜索工具</span>
                    <span className="block text-xs text-muted-foreground">{notes.join(" ")}</span>
                </span>
            </label>
            {error && (
                <div role="alert" className="flex items-start gap-2 rounded-md border border-border bg-muted/40 px-2.5 py-2 text-xs">
                    <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                    <span className="min-w-0 flex-1 break-words">
                        提供商已添加；接入搜索工具失败：{error}
                    </span>
                    <button type="button" className="shrink-0 text-muted-foreground underline underline-offset-2 hover:text-foreground" onClick={onSkip}>
                        跳过
                    </button>
                </div>
            )}
        </section>
    );
};

/** 类型卡片上的「本机已安装」「已添加」标识。 */
const TypeBadges: React.FC<{ installed: LocalAgentStatus | undefined; added: number }> = ({ installed, added }) => {
    const isInstalled = installed?.state === "installed";
    if (!isInstalled && added === 0) return null;
    return (
        <span className="mt-1 flex flex-wrap gap-1">
            {isInstalled && (
                <span className="inline-flex items-center gap-0.5 rounded border border-border px-1.5 py-px text-[11px] leading-4 text-muted-foreground">
                    <Check className="h-3 w-3" />本机已安装
                </span>
            )}
            {added > 0 && (
                <span className="inline-flex items-center rounded bg-muted px-1.5 py-px text-[11px] leading-4 text-muted-foreground">
                    已添加{added > 1 ? ` ${added} 个` : ""}
                </span>
            )}
        </span>
    );
};
