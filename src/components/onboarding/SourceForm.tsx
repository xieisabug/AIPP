import { useEffect, useRef, useState } from "react";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { createPortal } from "react-dom";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
    AlertCircle,
    ArrowRight,
    Check,
    ChevronDown,
    ChevronRight,
    ExternalLink,
    Globe,
    History,
    KeyRound,
    Plus,
    Share2,
    Terminal,
} from "lucide-react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { PasswordInput } from "@/components/ui/password-input";
import { Textarea } from "@/components/ui/textarea";
import NewProviderWizard from "@/components/config/NewProviderWizard";
import type { ModelSelectionResponse } from "@/components/config/llmModelTypes";
import { PROVIDER_API_TYPES, suggestProviderName } from "@/components/config/providerApiTypes";
import type { useLocalAgentDetection } from "@/components/config/useLocalAgentDetection";
import { parseProviderShareMessage } from "@/utils/providerShareMessage";
import type { OnboardingPreset } from "@/utils/modelRecommend";
import { prepareSource, type Checked, type ModelSource } from "./verifySources";
import {
    checkAgent,
    createAgentDraft,
    createDraft,
    deadline,
    detail,
    explain,
    fetchProviderModels,
    importProvider,
    local,
    loginAgent,
    openSettings,
    populateImported,
    providerList,
    providerSettings,
    type Path,
    type Provider,
} from "./onboardingService";

import { LieFlatModels } from "./LieFlatModels";

type Source = "share" | "key" | "agent" | "other" | "existing";
const apiTypes = PROVIDER_API_TYPES.filter((t) => t.kind !== "local");
type Detection = ReturnType<typeof useLocalAgentDetection>;
interface Props {
    footer: HTMLDivElement | null;
    path: Path;
    preset: OnboardingPreset;
    providers: Provider[];
    usable: Provider[];
    detection: Detection;
    proxy: boolean;
    recharge?: number | null;
    onChecked: (v: Checked) => Promise<void>;
    busy: boolean;
    run: (fn: () => Promise<void>) => Promise<void>;
    progress: (s: string) => void;
    refresh: () => Promise<void>;
    onNext: () => void;
    showingModels: boolean;
    onModels: () => void;
    onControls: (hidden: boolean) => void;
}
export function SourceForm({
    footer,
    path,
    preset,
    providers,
    usable,
    detection,
    proxy,
    recharge,
    onChecked,
    busy,
    run,
    progress,
    refresh,
    onNext,
    showingModels,
    onModels,
    onControls,
}: Props) {
    const expert = path === "expert";
    useDataRefresh({events:["llm_provider_changed"], paused:!expert, refresh:()=>refresh()});
    const recommended = preset.recommended_provider;
    const [source, setSource] = useState<Source>(recharge ? "existing" : "key");
    const [catalog, setCatalog] = useState(false);
    const [otherSource, setOtherSource] = useState<{ provider: Provider; models?: ModelSelectionResponse["available_models"] }>();
    const [removeProvider, setRemoveProvider] = useState<Provider>();
    const [share, setShare] = useState("");
    const [password, setPassword] = useState("");
    const [key, setKey] = useState("");
    const [optional, setOptional] = useState("");
    const [agent, setAgent] = useState<{ type: string; name: string; cli?: string }>();
    const [existing, setExisting] = useState<number | null>(recharge ?? null);
    const [modelSource, setModelSource] = useState<ModelSource>();
    const [failure, setFailure] = useState<{ kind?: string; message: string; raw: string }>();
    const [rechargeSaved, setRechargeSaved] = useState(!!recharge);
    const [reusable, setReusable] = useState<Provider[]>([]);
    const [added, setAdded] = useState<number[]>([]);
    const [login, setLogin] = useState<Provider>();
    const [balanceId, setBalanceId] = useState<number | null>(recharge ?? null);
    const cache = useRef(new Map<string, Provider>());
    const parsed = parseProviderShareMessage(share);
    const importedIds = useRef(new Set<number>());
    const agentTypes = [...PROVIDER_API_TYPES.filter((t) => t.kind === "local")].sort((a, b) => {
        const order = (s: string) => (s === "claude_sdk" ? 0 : s === "codex_app_server" ? 1 : 2);
        return order(a.value) - order(b.value);
    });
    const pick = (s: Source) => {
        setSource(s);
        setFailure(undefined);
        setModelSource(undefined);
        setLogin(undefined);
        setBalanceId(null);
        setRechargeSaved(false);
    };
    useEffect(() => {
        let active = true;
        const same = providers.filter((p) => p.api_type === recommended.api_type);
        void Promise.all(same.map(async (p) => ((await providerSettings(p.id)).api_key?.trim() ? p : null)))
            .then((list) => {
                if (active) setReusable(list.filter((p): p is Provider => !!p));
            })
            .catch((e) => {
                if (active) setFailure({ message: detail(e), raw: "" });
            });
        return () => {
            active = false;
        };
    }, [providers, recommended.api_type]);
    useEffect(() => {
        onControls(catalog);
        return () => onControls(false);
    }, [catalog, onControls]);
    function handleError(e: unknown, providerName = source === "other" ? otherSource?.provider.name ?? "服务商" : recommended.name) {
        setFailure({
            kind: e && typeof e === "object" && "kind" in e ? String(e.kind) : undefined,
            message: explain(e, providerName),
            raw: detail(e),
        });
    }
    async function submit() {
        setFailure(undefined);
        const within = deadline();
        try {
            let p = source === "other" ? otherSource?.provider : undefined;
            let agentProvider: Provider | undefined;
            async function apiKeyProvider(value: string) {
                const cached = cache.current.get("key:" + value);
                if (cached) return cached;
                const name = suggestProviderName(
                    recommended.name,
                    providers.map((p) => p.name)
                );
                const saved = await createDraft(name, recommended.api_type, {
                    endpoint: recommended.endpoint,
                    proxy_enabled: String(proxy),
                    api_key: value,
                });
                cache.current.set("key:" + value, saved);
                return saved;
            }
            if (!p && source === "share") {
                if (!parsed) throw new Error("分享码好像不完整，请让对方把整段消息重新复制给你");
                progress("正在解开分享码…");
                const imported = await within(() =>
                    importProvider(parsed.code, parsed.password || password, true)
                );
                if (imported.already_exists) toast.info("你已经有这个配置了");
                p = imported;
                if (!imported.already_exists) importedIds.current.add(p.id);
                await invoke("track_onboarding_provider", { providerId: p.id, owned: importedIds.current.has(p.id) });
                if (expert) {
                    if (local(imported.api_type)) {
                        progress(`正在验证 ${imported.name} 并获取模型列表…`);
                        await checkAgent(imported);
                    }
                    if (!imported.is_enabled)
                        await invoke("update_llm_provider", {
                            id: imported.id,
                            name: imported.name,
                            apiType: imported.api_type,
                            description: imported.description ?? "",
                            isEnabled: true,
                        });
                    await invoke("track_onboarding_provider", { providerId: imported.id, owned: false });
                    try {
                        await within(() => populateImported(imported, preset));
                    } catch (e) {
                        toast.warning(`已导入，但没能获取或保存模型列表：${detail(e)}`);
                    }
                    if (!imported.already_exists) setAdded((ids) => [...ids, imported.id]);
                    setShare("");
                    setPassword("");
                    await refresh();
                    return;
                }
            }
            if (!p && source === "agent" && agent) {
                const id = agent.type + ":" + (agent.cli ?? "");
                p = cache.current.get(id);
                if (!p) {
                    p = await within(() =>
                        createAgentDraft(
                            agent.type,
                            suggestProviderName(
                                agent.name,
                                providers.map((p) => p.name)
                            ),
                            agent.cli
                        )
                    );
                    cache.current.set(id, p);
                }
                agentProvider = p;
                setLogin(p);
                if (optional.trim()) p = await within(() => apiKeyProvider(optional.trim()));
            }
            if (!p && source === "existing" && existing)
                p = (await within(providerList)).find((i) => i.id === existing);
            if (!p && source === "key" && key.trim()) p = await within(() => apiKeyProvider(key.trim()));
            if (!p) throw new Error("请选择一个 AI 来源");
            if (p.api_type === recommended.api_type && (source === "key" || (recharge && p.id === recharge))) {
                setBalanceId(p.id);
            }
            if (!local(p.api_type) && (!p.is_enabled || (recharge && p.id === recharge))) {
                await invoke("update_llm_provider_config", {
                    llmProviderId: p.id,
                    name: "proxy_enabled",
                    value: String(proxy),
                });
            }
            const result = await prepareSource(p, preset, within, progress, {
                agent: agentProvider,
                models: source === "other" ? otherSource?.models : undefined,
            });
            if ("codes" in result) {
                setModelSource(result);
                onModels();
                return;
            }
            await onChecked(result);
        } catch (e) {
            handleError(e);
        }
    }
    const ready = source === "key"
          ? !!key.trim()
          : source === "share"
            ? !!share.trim() && !!(parsed?.password || password.trim())
            : source === "existing"
              ? !!existing
              : source === "other"
                ? !!otherSource
                : source === "agent" && !!agent;
    if (showingModels && modelSource) {
        return <LieFlatModels source={modelSource} footer={footer} busy={busy} run={run} progress={progress} onChecked={onChecked} />;
    }
    if (catalog)
        return (
            <NewProviderWizard
                embedded={path}
                open
                apiTypes={expert ? PROVIDER_API_TYPES : apiTypes}
                existingProviders={providers}
                onClose={() => setCatalog(false)}
                onCreated={(id) => {
                    setCatalog(false);
                    if (expert) {
                        setAdded((ids) => [...ids, id]);
                        void refresh().catch(handleError);
                    } else
                        void run(async () => {
                            setOtherSource(undefined);
                            let providerName = "服务商";
                            try {
                                const within = deadline();
                                const provider = (await within(providerList)).find((p) => p.id === id);
                                if (!provider) throw new Error("找不到刚保存的提供商");
                                providerName = provider.name;
                                setOtherSource({ provider });
                                progress("正在获取模型列表…");
                                const models = await within(() => fetchProviderModels(id));
                                setOtherSource({ provider, models });
                            } catch (error) {
                                handleError(error, providerName);
                            }
                        });
                }}
            />
        );
    const shareField = (
        <div className="space-y-2.5">
            <Textarea
                aria-label="分享消息"
                className="min-h-24 resize-none text-sm"
                placeholder="把对方发来的整段消息粘贴到这里…"
                value={share}
                onChange={(e) => setShare(e.target.value)}
            />
            {parsed && !parsed.password && (
                <PasswordInput
                    aria-label="分享密码"
                    placeholder="输入分享密码"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                />
            )}
            {share.trim() && !parsed && (
                <p className="text-xs text-amber-700 dark:text-amber-400">
                    未识别到 AIPP 分享码，请确认已复制整段分享消息或完整分享码。
                </p>
            )}
        </div>
    );
    const selectSource = (
        s: Source,
        title: string,
        children: React.ReactNode,
        options?: { icon?: React.ReactNode; subtitle?: string }
    ) => {
        const active = source === s;
        return (
            <section
                className={`rounded-2xl transition-all duration-200 ${
                    active
                        ? "bg-muted/80 ring-1 ring-foreground/20 dark:bg-muted/50"
                        : "bg-muted/55 hover:bg-muted/80 dark:bg-muted/30 dark:hover:bg-muted/45"
                }`}
            >
                <button
                    type="button"
                    aria-expanded={active}
                    className="flex w-full items-center gap-3 px-4 py-3.5 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                    onClick={() => pick(s)}
                >
                    {options?.icon && (
                        <span
                            className={`flex h-8 w-8 shrink-0 items-center justify-center rounded-xl transition-colors ${
                                active ? "bg-foreground text-background" : "bg-background/80 text-muted-foreground"
                            }`}
                        >
                            {options.icon}
                        </span>
                    )}
                    <span className="min-w-0 flex-1">
                        <span className="block text-sm font-medium text-foreground">{title}</span>
                        {!active && options?.subtitle && (
                            <span className="mt-0.5 block truncate text-xs text-muted-foreground">
                                {options.subtitle}
                            </span>
                        )}
                    </span>
                    <span
                        aria-hidden="true"
                        className={`flex h-4 w-4 shrink-0 items-center justify-center rounded-full border transition-colors ${
                            active ? "border-foreground bg-foreground text-background" : "border-muted-foreground/35"
                        }`}
                    >
                        {active && <Check className="h-2.5 w-2.5" strokeWidth={3} />}
                    </span>
                </button>
                {active && <div className="space-y-3 px-4 pb-4 pt-1">{children}</div>}
            </section>
        );
    };
    return (
        <form
            id="onboarding-source"
            className="space-y-3"
            onSubmit={(e) => {
                e.preventDefault();
                if (expert && !busy && ready) void run(() => submit());
            }}
        >
            <fieldset className="min-w-0 space-y-3" disabled={busy}>
                {expert ? (
                    <div className="space-y-3.5">
                        <div className="rounded-2xl bg-muted/60 dark:bg-muted/35">
                            <div className="flex items-center justify-between border-b border-border/40 px-5 py-3">
                                <h3 className="text-sm font-medium">已添加的提供商</h3>
                                <Button variant="ghost" size="sm" type="button" className="h-8 gap-1.5 rounded-full bg-background/80 px-3.5 text-xs hover:bg-background" onClick={() => setCatalog(true)}>
                                    <Plus className="h-3.5 w-3.5" />
                                    添加提供商
                                </Button>
                            </div>
                            <div className="divide-y divide-border/40 px-5">
                                {providers
                                    .filter((p) => p.is_enabled || added.includes(p.id))
                                    .map((p) => {
                                        const isUsable = usable.some((item) => item.id === p.id);
                                        return (
                                            <div className="flex items-center justify-between gap-2 py-2.5 text-sm" key={p.id}>
                                                <div className="flex min-w-0 items-center gap-2">
                                                    <span className="truncate font-medium">{p.name}</span>
                                                    <span
                                                        className={`rounded-full px-2 py-0.5 text-[11px] ${
                                                            isUsable
                                                                ? "bg-emerald-500/10 text-emerald-700 dark:text-emerald-400"
                                                                : "bg-amber-500/10 text-amber-700 dark:text-amber-400"
                                                        }`}
                                                    >
                                                        {isUsable ? "可用" : "待配置模型"}
                                                    </span>
                                                </div>
                                                <div className="flex shrink-0 items-center gap-1">
                                                    {!isUsable && (
                                                        <Button
                                                            type="button"
                                                            size="sm"
                                                            variant="link"
                                                            className="h-7 px-2 text-xs"
                                                            onClick={() => void run(() => openSettings("llm-provider-config", String(p.id)))}
                                                        >
                                                            配置模型
                                                        </Button>
                                                    )}
                                                    {added.includes(p.id) && (
                                                        <Button
                                                            type="button"
                                                            size="sm"
                                                            variant="ghost"
                                                            className="h-7 px-2 text-xs text-muted-foreground hover:text-destructive"
                                                            onClick={() => setRemoveProvider(p)}
                                                        >
                                                            移除
                                                        </Button>
                                                    )}
                                                </div>
                                            </div>
                                        );
                                    })}
                                {!providers.some((p) => p.is_enabled || added.includes(p.id)) && (
                                    <p className="py-5 text-center text-xs text-muted-foreground">
                                        暂无已启用的提供商，点击右上角「添加提供商」或下方导入分享码。
                                    </p>
                                )}
                            </div>
                        </div>
                        {removeProvider && (
                            <div role="alert" className="flex items-center justify-between gap-2 rounded-xl border border-destructive/30 bg-destructive/5 px-3.5 py-2.5 text-sm">
                                <span>删除刚添加的「{removeProvider.name}」？</span>
                                <div className="flex items-center gap-1.5">
                                    <Button
                                        type="button"
                                        size="sm"
                                        variant="outline"
                                        className="h-7 px-2.5 text-xs"
                                        onClick={() => setRemoveProvider(undefined)}
                                    >
                                        取消
                                    </Button>
                                    <Button
                                        type="button"
                                        size="sm"
                                        variant="destructive"
                                        className="h-7 px-2.5 text-xs"
                                        onClick={() =>
                                            void run(async () => {
                                                await invoke("delete_llm_provider", {
                                                    llmProviderId: removeProvider.id,
                                                });
                                                setAdded((ids) => ids.filter((id) => id !== removeProvider.id));
                                                setRemoveProvider(undefined);
                                                await refresh();
                                            })
                                        }
                                    >
                                        确认删除
                                    </Button>
                                </div>
                            </div>
                        )}
                        {!usable.length && providers.some((p) => p.is_enabled || added.includes(p.id)) && (
                            <p className="text-xs text-muted-foreground">
                                请添加并启用至少一个带聊天模型的 API 或本机智能体后继续。
                            </p>
                        )}
                        {selectSource(
                            "share",
                            "导入分享码",
                            <>
                                {shareField}
                                <div className="flex justify-end">
                                    <Button
                                        type="button"
                                        size="sm"
                                        disabled={!ready || busy}
                                        onClick={() => void run(() => submit())}
                                    >
                                        导入
                                    </Button>
                                </div>
                            </>,
                            { icon: <Share2 className="h-4 w-4" />, subtitle: "粘贴好友发来的 AIPP 配置分享消息" }
                        )}
                    </div>
                ) : (
                    <>
                        {agentTypes.some((t) => detection.statuses[t.value]?.state === "installed") && (
                            <section
                                className={`rounded-2xl p-4 transition-all duration-200 ${
                                    source === "agent"
                                        ? "bg-muted/80 ring-1 ring-foreground/20 dark:bg-muted/50"
                                        : "bg-muted/55 dark:bg-muted/30"
                                }`}
                            >
                                <div className="mb-3 flex items-center justify-between">
                                    <div className="flex items-center gap-2">
                                        <Terminal className="h-4 w-4 text-muted-foreground" />
                                        <h3 className="text-sm font-medium">你电脑上已经有</h3>
                                    </div>
                                    <span className="text-xs text-muted-foreground">点选即可直接使用</span>
                                </div>
                                <div className="flex flex-wrap gap-2">
                                    {agentTypes.flatMap((t) => {
                                        const status = detection.statuses[t.value];
                                        if (status?.state !== "installed") return [];
                                        const needsNode = !!status.node && !status.node.ok;
                                        const choices =
                                            t.value === "acp"
                                                ? (detection.acpClis ?? [])
                                                      .filter((c) => c.installed)
                                                      .map((c) => ({ name: c.label, cli: c.value }))
                                                : [{ name: t.label, cli: undefined }];
                                        return choices.map((c) => {
                                            const active = source === "agent" && agent?.name === c.name;
                                            return (
                                                <div key={t.value + c.name} className="flex items-center gap-1.5">
                                                    <Button
                                                        type="button"
                                                        size="sm"
                                                        disabled={needsNode}
                                                        variant={active ? "default" : "ghost"}
                                                        className={`h-9 gap-1.5 rounded-full px-4 text-xs font-medium ${
                                                            active ? "" : "bg-background/80 hover:bg-background"
                                                        }`}
                                                        onClick={() => {
                                                            pick("agent");
                                                            setAgent({ type: t.value, ...c });
                                                        }}
                                                    >
                                                        <span>{c.name} · 已安装</span>
                                                    </Button>
                                                    {needsNode && (
                                                        <Button
                                                            type="button"
                                                            size="sm"
                                                            variant="link"
                                                            className="h-7 px-1 text-xs"
                                                            onClick={() => void openUrl("https://nodejs.org/")}
                                                        >
                                                            需要先安装 Node.js
                                                        </Button>
                                                    )}
                                                </div>
                                            );
                                        });
                                    })}
                                </div>
                                {source === "agent" && (
                                    <div className="mt-3.5 space-y-2 rounded-lg border border-border/70 bg-muted/25 p-3">
                                        <label className="block space-y-1.5 text-xs font-medium text-foreground">
                                            <span>再加一个 {recommended.name} Key（可选）</span>
                                            <PasswordInput
                                                value={optional}
                                                onChange={(e) => setOptional(e.target.value)}
                                                placeholder={recommended.price_hint || "粘贴 API Key（可留空）"}
                                            />
                                        </label>
                                        <p className="text-xs text-muted-foreground">
                                            引导会检查本机配置并读取智能体的模型列表。可选用其中的型号处理轻量任务，也可填写 Key，使用 API 处理轻量任务和工具审核。
                                        </p>
                                    </div>
                                )}
                            </section>
                        )}
                        {selectSource(
                            "key",
                            `我有 ${recommended.name} 的 Key`,
                            <>
                                <PasswordInput
                                    aria-label="API Key"
                                    placeholder="粘贴 API Key（如 sk-...）"
                                    value={key}
                                    onChange={(e) => setKey(e.target.value)}
                                />
                                <details className="group rounded-lg bg-muted/40 px-3 py-2 text-xs">
                                    <summary className="flex cursor-pointer list-none items-center gap-1.5 font-medium text-muted-foreground hover:text-foreground">
                                        <ChevronRight className="h-3.5 w-3.5 transition-transform group-open:rotate-90" />
                                        还没有？{recommended.key_guide.length} 步获取
                                    </summary>
                                    <ol className="mt-2 space-y-1.5 pl-5 list-decimal text-foreground">
                                        {recommended.key_guide.map((step) => (
                                            <li key={step.url}>
                                                <button
                                                    type="button"
                                                    className="inline-flex items-center gap-1 underline underline-offset-2 hover:text-foreground/80"
                                                    onClick={() => void openUrl(step.url)}
                                                >
                                                    {step.title}
                                                    <ExternalLink className="h-3 w-3 text-muted-foreground" />
                                                </button>
                                            </li>
                                        ))}
                                    </ol>
                                </details>
                            </>,
                            {
                                icon: <KeyRound className="h-4 w-4" />,
                                subtitle: recommended.price_hint || "自动配好主力、打杂与联网搜索",
                            }
                        )}
                        {selectSource(
                            "share",
                            "有人发给我分享码",
                            shareField,
                            {
                                icon: <Share2 className="h-4 w-4" />,
                                subtitle: "整段粘贴好友转发的配置消息即可自动识别",
                            }
                        )}
                        {(reusable.length > 0 || !!recharge) &&
                            selectSource(
                                "existing",
                                recharge ? "继续检查已保存的配置" : `你已经添加过 ${recommended.name}，直接用它？`,
                                <div className="relative">
                                    <select
                                        aria-label="已有提供商"
                                        className="h-9 w-full appearance-none rounded-md border border-input bg-background pl-3 pr-8 text-sm shadow-xs focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
                                        value={existing ?? ""}
                                        onChange={(e) => setExisting(Number(e.target.value))}
                                    >
                                        <option value="">选择提供商</option>
                                        {providers
                                            .filter((p) => p.id === recharge || reusable.some((r) => r.id === p.id))
                                            .map((p) => (
                                                <option key={p.id} value={p.id}>
                                                    {p.name}
                                                </option>
                                            ))}
                                    </select>
                                    <ChevronDown className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
                                </div>,
                                {
                                    icon: <History className="h-4 w-4" />,
                                    subtitle: recharge ? "充值完成后点击下方重新检查" : "复用已填好 Key 的服务商配置",
                                }
                            )}
                        {otherSource ? selectSource("other", otherSource.provider.name, (
                            <div className="flex items-center justify-between gap-3">
                                {otherSource.models && (
                                    <span className="text-xs text-muted-foreground">已获取 {otherSource.models.length} 个模型</span>
                                )}
                                <Button type="button" variant="outline" size="sm" onClick={() => setCatalog(true)}>
                                    更换提供商
                                </Button>
                            </div>
                        ), { icon: <Globe className="h-4 w-4" /> }) : <button
                            type="button"
                            className="group flex w-full items-center gap-3 rounded-2xl bg-muted/45 px-4 py-3 text-left text-sm transition-colors hover:bg-muted/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:bg-muted/25 dark:hover:bg-muted/45"
                            onClick={() => {
                                pick("other");
                                setCatalog(true);
                            }}
                        >
                            <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-background/80 text-muted-foreground">
                                <Globe className="h-4 w-4" />
                            </span>
                            <span className="flex-1">
                                <span className="block font-medium text-foreground">我用的是其他服务商</span>
                                <span className="block text-xs text-muted-foreground">OpenAI、Anthropic、Gemini、Ollama 等</span>
                            </span>
                            <ArrowRight className="h-4 w-4 text-muted-foreground/70 transition-transform group-hover:translate-x-0.5" />
                        </button>}
                    </>
                )}
                {failure && (
                    <div
                        role="alert"
                        className="space-y-2.5 rounded-xl border border-amber-500/40 bg-amber-500/5 p-3.5 text-sm"
                    >
                        <div className="flex items-start gap-2.5">
                            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                            <p className="font-medium text-foreground">{failure.message}</p>
                        </div>
                        <div className="flex flex-wrap items-center gap-2 pl-6">
                            {(failure.kind === "network" || failure.kind === "timeout") && (
                                <Button
                                    variant="outline"
                                    size="sm"
                                    type="button"
                                    className="h-7 text-xs"
                                    onClick={() => void openSettings("network-config")}
                                >
                                    代理设置
                                </Button>
                            )}
                            {failure.kind === "login" && login && (
                                <Button
                                    type="button"
                                    size="sm"
                                    className="h-7 text-xs"
                                    onClick={() =>
                                        void run(async () => {
                                            await loginAgent(login);
                                            await submit();
                                        })
                                    }
                                >
                                    登录 {login.name}
                                </Button>
                            )}
                            {failure.kind === "insufficient_balance" && (
                                <>
                                    <Button
                                        type="button"
                                        size="sm"
                                        variant="outline"
                                        className="h-7 gap-1 text-xs"
                                        onClick={() =>
                                            void openUrl(
                                                recommended.key_guide.find((s) => s.title.includes("充值"))?.url ??
                                                    "https://platform.deepseek.com/top_up"
                                            )
                                        }
                                    >
                                        去充值
                                        <ExternalLink className="h-3 w-3" />
                                    </Button>
                                    {balanceId && !rechargeSaved && (
                                        <Button
                                            variant="secondary"
                                            size="sm"
                                            type="button"
                                            className="h-7 text-xs"
                                            onClick={() =>
                                                void run(async () => {
                                                    await invoke("save_onboarding_recharge", {
                                                        providerId: balanceId,
                                                    });
                                                    setRechargeSaved(true);
                                                })
                                            }
                                        >
                                            保存配置，稍后充值
                                        </Button>
                                    )}
                                </>
                            )}
                        </div>
                        {failure.raw && failure.raw !== failure.message && (
                            <details className="pl-6 text-xs text-muted-foreground">
                                <summary className="cursor-pointer hover:text-foreground">查看原始信息</summary>
                                <pre className="mt-1.5 whitespace-pre-wrap break-all rounded-md bg-muted/50 p-2 font-mono text-[11px]">
                                    {failure.raw}
                                </pre>
                            </details>
                        )}
                    </div>
                )}
                {rechargeSaved && (
                    <p
                        role="status"
                        className="flex items-center gap-2 rounded-lg border border-emerald-500/30 bg-emerald-500/5 px-3 py-2 text-xs text-emerald-700 dark:text-emerald-400"
                    >
                        <Check className="h-3.5 w-3.5 shrink-0" />
                        已保存提供商和 Key。充值后可在这里重新检查。
                    </p>
                )}
            </fieldset>
            {footer &&
                createPortal(
                    <Button
                        data-primary
                        type="button"
                        className="rounded-full px-6"
                        disabled={busy || (expert ? !usable.length : !ready)}
                        onClick={expert ? onNext : () => void run(() => submit())}
                    >
                        {failure?.kind === "insufficient_balance" || rechargeSaved
                            ? "我已充值，重新检查"
                            : "下一步"}
                    </Button>,
                    footer
                )}
        </form>
    );
}
