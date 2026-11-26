import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openPath } from "@tauri-apps/plugin-opener";
import {
    AlertCircle,
    Check,
    CheckCircle2,
    ChevronLeft,
    Cloud,
    Cpu,
    FileText,
    FolderOpen,
    Globe,
    Loader2,
    RefreshCw,
    Sparkles,
    Terminal,
} from "lucide-react";
import { WindowTopBar } from "@/components/window/WindowTitleBar";
import { useTheme } from "@/hooks/useTheme";
import { Button } from "@/components/ui/button";
import {
    Dialog,
    CompactDialogContent,
    CompactDialogHeader,
    CompactDialogBody,
    CompactDialogFooter,
} from "@/components/ui/dialog";
import AnimatedAvatar from "@/components/avatar/AnimatedAvatar";
import { defaultAppearance } from "@/components/butler/butlerAppearance";
import { PROVIDER_API_TYPES } from "@/components/config/providerApiTypes";
import { useLocalAgentDetection } from "@/components/config/useLocalAgentDetection";
import { useFeatureConfig } from "@/hooks/feature/useFeatureConfig";
import type { ModelForSelect } from "@/hooks/useFilteredModels";
import { getOfficePreviewPluginStatus, isOfficeEditPluginReady } from "@/services/officePreviewPlugin";
import { useVaultItems } from "@/hooks/useVaultItems";
import { connectDeepSeekSearch, hideKey } from "@/components/config/deepseekSuite";
import { SourceForm } from "@/components/onboarding/SourceForm";
import { HomeFields, ModelRoles } from "@/components/onboarding/ModelRoles";
import {
    modelRoles, rankedRoles, selectRecommended, verifyRecommendedRoles,
} from "@/components/onboarding/roleRecommendations";
import { SetupList, setupNames, useSetupItems, type SetupId } from "@/components/onboarding/OnboardingSetup";
import { checkSelected, type Checked, type Notes } from "@/components/onboarding/verifySources";
import {
    askModel,
    deadline,
    detail,
    explain,
    local,
    openSettings,
    probeSession,
    loginAgent,
    providerList,
    providerSettings,
    requiresKey,
    role,
    roleKey,
    roleOf,
    type Config,
    type OnboardingState,
    type Path,
    type Provider,
} from "@/components/onboarding/onboardingService";
import type { OnboardingPreset } from "@/utils/modelRecommend";

type Page = "welcome" | "sources" | "roles" | "home" | "extras" | "applying" | "done";
const allExtras = Object.keys(setupNames) as SetupId[];
const extraInfo = {
    skills: "扩展助手可用的技能",
    office: "预览和编辑 Word、Excel、PPT",
    bun: "运行 React 和 Vue 组件预览",
    uv: "运行 Python 脚本、管理依赖",
};
const extraIcons: Record<SetupId, React.ReactNode> = {
    skills: <Sparkles className="h-4 w-4 text-muted-foreground" />,
    office: <FileText className="h-4 w-4 text-muted-foreground" />,
    bun: <Cpu className="h-4 w-4 text-muted-foreground" />,
    uv: <Terminal className="h-4 w-4 text-muted-foreground" />,
};
const titles: Record<Page, string> = {
    welcome: "欢迎使用 AIPP",
    sources: "你的 AI 从哪来？",
    roles: "模型分工",
    home: "选择主界面",
    extras: "顺手装上这些？",
    applying: "正在应用配置",
    done: "好了，可以开始了",
};
const initial: Config = { chat: role(0, ""), home: "chat_ui", extras: allExtras };

function PathCard({ path, delayMs, onChoose }: { path: Path; delayMs: number; onChoose: () => void }) {
    const expert = path === "expert";
    const [seq, setSeq] = useState(0);
    return (
        <button
            type="button"
            style={{ "--stagger-delay": `${delayMs}ms` } as CSSProperties}
            className="aipp-onboarding-stagger group relative flex h-64 flex-col items-center justify-center rounded-3xl bg-muted/65 px-7 text-center transition-all duration-300 hover:-translate-y-0.5 hover:bg-muted hover:shadow-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring dark:bg-muted/40 dark:hover:bg-muted/65"
            onMouseEnter={() => setSeq((n) => n + 1)}
            onFocus={() => setSeq((n) => n + 1)}
            onClick={onChoose}
        >
            <span className="transition-transform duration-300 group-hover:-translate-y-1">
                <AnimatedAvatar
                    animated
                    size={96}
                    appearance={{
                        ...defaultAppearance,
                        accessories: {
                            eyes: expert ? "mustache" : "sunglasses",
                            head: null,
                        },
                    }}
                    actionRequest={seq ? { action: expert ? "twirl" : "peer", sequence: seq } : undefined}
                />
            </span>
            <strong className="mt-3 text-xl font-medium tracking-tight text-foreground">
                {expert ? "专家" : "躺平"}
            </strong>
            <div className="grid grid-rows-[0fr] opacity-0 transition-all duration-300 group-hover:grid-rows-[1fr] group-hover:opacity-100 group-focus-visible:grid-rows-[1fr] group-focus-visible:opacity-100">
                <span className="overflow-hidden pt-2 text-xs leading-relaxed text-muted-foreground">
                    {expert
                        ? "自由挑选模型分工、主界面与附加组件"
                        : "只需一个 AI 来源，其余按推荐自动配好"}
                </span>
            </div>
        </button>
    );
}
export default function OnboardingWindow() {
    useTheme("onboarding");
    const [page, setPage] = useState<Page>("welcome");
    const [path, setPath] = useState<Path>("lie_flat");
    const [status, setStatus] = useState<OnboardingState>();
    const [preset, setPreset] = useState<OnboardingPreset>();
    const [config, setConfig] = useState<Config>(initial);
    const [notes, setNotes] = useState<Notes>({});
    const [providers, setProviders] = useState<Provider[]>([]);
    const [models, setModels] = useState<ModelForSelect[]>([]);
    const [acpCommands, setAcpCommands] = useState<Record<number, string>>({});
    const [ask, setAsk] = useState<string | null>(null);
    const [searchStatus, setSearchStatus] = useState("");
    const [busy, setBusy] = useState(false);
    const lock = useRef(false);
    const [progress, setProgress] = useState("");
    const [error, setError] = useState("");
    const [loginId, setLoginId] = useState<number | null>(null);
    const [waiting, setWaiting] = useState(false);
    const [syncMessage, setSyncMessage] = useState("");
    const [embedded, setEmbedded] = useState(false);
    const [editing, setEditing] = useState(false);
    const [addingApi, setAddingApi] = useState(false);
    const [extraChecks, setExtraChecks] = useState<Partial<Record<SetupId, boolean | string>>>({});
    const [extraCheckAttempt, setExtraCheckAttempt] = useState(0);
    const [pending, setPending] = useState<{ checked: Checked; kind: "rerun" | "channel" }>();
    const [bootRetry, setBootRetry] = useState(0);
    const [sourceRevision, setSourceRevision] = useState(0);
    const sourceGeneration = useRef(0);
    const detection = useLocalAgentDetection(PROVIDER_API_TYPES);
    const { getConfigValue } = useFeatureConfig();
    const vault = useVaultItems();
    const items = useSetupItems();
    const [sourceButton, setSourceButton] = useState<HTMLDivElement | null>(null);
    const run = useCallback(async (fn: () => Promise<void>) => {
        if (lock.current) return;
        lock.current = true;
        setLoginId(null);
        setBusy(true);
        setError("");
        setProgress("");
        try {
            await fn();
        } catch (e) {
            if (e && typeof e === "object" && "kind" in e && e.kind === "login" && "providerId" in e)
                setLoginId(Number(e.providerId));
            setError(explain(e));
        } finally {
            lock.current = false;
            setBusy(false);
            setProgress("");
        }
    }, []);
    const refresh = useCallback(async () => {
        const rev = ++sourceGeneration.current;
        const [list, api, agents, currentAsk] = await Promise.all([
            providerList(),
            invoke<ModelForSelect[]>("get_filtered_models_for_select", { assistantType: 0 }),
            invoke<ModelForSelect[]>("get_filtered_models_for_select", { assistantType: 4 }),
            askModel(),
        ]);
        const eligible = new Set<number>();
        const commands: Record<number, string> = {};
        await Promise.all(
            list
                .filter((p) => p.is_enabled)
                .map(async (p) => {
                    if (local(p.api_type) || !requiresKey(p.api_type)) eligible.add(p.id);
                    if (p.api_type === "acp" || requiresKey(p.api_type)) {
                        const settings = await providerSettings(p.id);
                        if (settings.api_key?.trim()) eligible.add(p.id);
                        if (p.api_type === "acp") commands[p.id] = settings.acp_cli_command || "";
                    }
                })
        );
        if (rev !== sourceGeneration.current) return;
        setProviders(list);
        setAcpCommands(commands);
        setModels([...api, ...agents].filter((m) => eligible.has(m.llm_provider_id)));
        setAsk(currentAsk);
    }, []);
    const installed = (p: Provider) => {
        const s = detection.statuses[p.api_type];
        if (s?.state !== "installed" || (s.node && !s.node.ok)) return false;
        return (
            p.api_type !== "acp" || !!detection.acpClis?.some((c) => c.installed && c.value === acpCommands[p.id])
        );
    };
    const usable = providers.filter((p) => models.some((m) => m.llm_provider_id === p.id));
    const label = (value: string | null | undefined) => {
        if (!value) return "未配置";
        const split = value.lastIndexOf("%%");
        if (split < 0) return value;
        const p = providers.find((p) => p.id === Number(value.slice(split + 2)));
        const code = value.slice(0, split);
        return [p?.name, code === "__agent_default__" || code === "acp" ? "默认模型" : code]
            .filter(Boolean)
            .join(" / ");
    };
    async function restorePendingApply() {
        const saved = await invoke<OnboardingState>("get_onboarding_state");
        if (saved.step === "source_ready" && saved.draft.config) {
            setStatus(saved);
            setConfig(saved.draft.config);
            setNotes(saved.draft.config.notes ?? {});
            setPage("applying");
        }
    }
    async function updateRoles(changes: Partial<Config>) {
        try {
            return await invoke<OnboardingState>("update_onboarding_roles", changes);
        } catch (error) {
            await restorePendingApply();
            throw error;
        }
    }
    async function apply(checked: Checked, confirmed = false, selectionPreset = preset) {
        let applied: OnboardingState;
        try {
            applied = await invoke<OnboardingState>("apply_onboarding_config", {
                config: {
                    ...checked.config,
                    notes: checked.notes,
                    confirmChannelChange: confirmed || checked.config.confirmChannelChange,
                },
            });
        } catch (e) {
            if (detail(e).includes("butler_channel_change_confirmation_required")) {
                setPending({ kind: "channel", checked });
                return;
            }
            await restorePendingApply();
            throw e;
        }
        const actual = applied.draft.config ?? checked.config;
        setStatus(applied);
        const appliedNotes = actual.notes ?? checked.notes;
        setConfig({ ...actual, notes: appliedNotes });
        setNotes(appliedNotes);
        setEditing(false);
        setAddingApi(false);
        setPage("done");
        await refresh();
        if (
            !editing &&
            !addingApi &&
            selectionPreset?.recommended_provider.extras?.enable_deepseek_search &&
            (applied.path ?? path) === "lie_flat"
        ) {
            const list = await providerList();
            const p = list.find(
                (p) =>
                    p.api_type === "deepseek" &&
                    [actual.chat, actual.light, actual.review].some((r) => r?.providerId === p.id && r.verified)
            );
            if (p) {
                const settings = await providerSettings(p.id);
                if (settings.api_key) {
                    // 搜索接入与后台安装都不阻塞用户打开首页。
                    void connectDeepSeekSearch({
                        providerId: p.id,
                        providerName: p.name,
                        apiKey: settings.api_key,
                        endpoint: settings.endpoint,
                        vault,
                    })
                        .then(() => setSearchStatus("已开启（DeepSeek）"))
                        .catch((e) => setSearchStatus("接入失败：" + hideKey(detail(e), settings.api_key)));
                }
            }
        }
    }
    async function accept(checked: Checked) {
        if (status?.rerun && !editing) {
            setPending({ checked, kind: "rerun" });
            return;
        }
        await apply(checked);
    }
    useEffect(() => {
        let active = true;
        void Promise.all([
            invoke<OnboardingState>("get_onboarding_state"),
            invoke<OnboardingPreset>("get_onboarding_preset"),
        ])
            .then(async ([state, data]) => {
                if (!active) return;
                setStatus(state);
                setPreset(data);
                setConfig({ ...initial, workspacePath: state.workspacePath });
                const mode = state.path ?? "lie_flat";
                setPath(mode);
                if (state.step === "source_ready" && state.draft.config) {
                    setConfig(state.draft.config);
                    setPage("applying");
                    await run(() =>
                        apply({ config: state.draft.config!, notes: state.draft.config?.notes ?? {} }, false, data)
                    );
                    return;
                }
                if (state.step === "applied" && !state.rerun && state.draft.config) {
                    setPage("done");
                    setConfig(state.draft.config);
                    setNotes(state.draft.config.notes ?? {});
                } else if (
                    state.path &&
                    (state.state === "in_progress" ||
                        state.step === "path_chosen" ||
                        state.step === "awaiting_recharge")
                )
                    setPage("sources");
                await refresh();
            })
            .catch((e) => {
                if (active) setError(detail(e));
            });
        return () => {
            active = false;
        };
    }, [bootRetry]);
    useEffect(() => {
        if (page !== "extras") return;
        let generation = 0;
        const pending = new Map<SetupId, Promise<boolean>>();
        const check = () => {
            const current = ++generation;
            setExtraChecks({});
            for (const id of allExtras) {
                const report = (result: boolean | string) => {
                    if (current === generation) setExtraChecks(previous => ({ ...previous, [id]: result }));
                };
                let probe = pending.get(id);
                if (!probe) {
                    probe = (id === "skills" ? invoke<boolean>("check_onboarding_skills_ready")
                        : id === "office" ? getOfficePreviewPluginStatus().then(isOfficeEditPluginReady)
                        : invoke<string>(`check_${id}_version`).then(version => Boolean(version && version !== "Not Installed")))
                        .finally(() => pending.delete(id));
                    pending.set(id, probe);
                }
                void probe.then(report, error => report(detail(error)));
            }
        };
        check();
        window.addEventListener("focus", check);
        return () => {
            ++generation;
            window.removeEventListener("focus", check);
        };
    }, [page, extraCheckAttempt]);
    const syncCheck = useRef<(s: SyncStatus) => Promise<void>>(async () => {});
    interface SyncStatus {
        mode: string;
        syncing: boolean;
        last_sync_at?: string | null;
        last_error?: string | null;
        connected: boolean;
    }
    syncCheck.current = async (s) => {
        if (s.mode === "local" || s.syncing) {
            setSyncMessage("");
            return;
        }
        if (s.last_error) {
            setSyncMessage(s.last_error);
            return;
        }
        if (!s.last_sync_at || !detectionReady) {
            setSyncMessage("");
            return;
        }
        const list = await providerList();
        const installedProviderIds: number[] = [];
        for (const p of list) {
            if (p.api_type === "acp") {
                const settings = await providerSettings(p.id);
                if (detection.acpClis?.some((c) => c.installed && c.value === settings.acp_cli_command))
                    installedProviderIds.push(p.id);
            } else if (installed(p)) installedProviderIds.push(p.id);
        }
        const res = await invoke<{ completed: boolean; reason: string | null; settingsPage: string | null }>(
            "check_onboarding_sync",
            { installedProviderIds }
        );
        if (res.settingsPage) {
            await openSettings(
                res.settingsPage === "data_sync" ? "data-sync-config" : "llm-provider-config",
                undefined,
                res.reason ?? undefined
            );
            await invoke("finish_onboarding", { settings: true });
        } else if (!res.completed) {
            setSyncMessage(res.reason || "未检测到可用来源");
            await refresh();
        }
    };
    const detectionReady =
        Object.values(detection.statuses).length > 0 &&
        Object.values(detection.statuses).every((s) => s.state !== "detecting");
    useEffect(() => {
        if (!waiting) return;
        let active = true,
            running = false;
        let latest: SyncStatus | undefined;
        async function check(s: SyncStatus) {
            latest = s;
            if (running) return;
            running = true;
            try {
                while (latest && active) {
                    const current = latest;
                    latest = undefined;
                    await syncCheck.current(current);
                }
            } catch (e) {
                if (active) setSyncMessage(detail(e));
            } finally {
                running = false;
            }
        }
        let receivedEvent = false;
        const sub = listen<SyncStatus>("sync_status_changed", (e) => {
            receivedEvent = true;
            if (active) void check(e.payload);
        });
        void sub
            .then(() => invoke<SyncStatus>("get_sync_status"))
            .then((s) => {
                if (active && !receivedEvent) void check(s);
            })
            .catch((e) => {
                if (active) setSyncMessage(detail(e));
            });
        return () => {
            active = false;
            void sub.then((stop) => stop());
        };
    }, [waiting, detectionReady]);
    useEffect(() => {
        setWaiting(false);
        setSyncMessage("");
    }, [page, embedded]);
    async function choose(next: Path) {
        await invoke("choose_onboarding_path", { path: next });
        setStatus(await invoke("get_onboarding_state"));
        setPath(next);
        setPage("sources");
    }
    function beginRoles() {
        if (!addingApi && !editing) {
            const rec = rankedRoles(providers, models, preset!);
            const agent = models.find(model => providers.some(p =>
                p.id === model.llm_provider_id && local(p.api_type) && installed(p)
            ));
            let defaults = initial;
            if (!rec.chat.length && agent) {
                defaults = selectRecommended(defaults, "chat", { model: roleOf(agent), reason: "本机已安装智能体" });
            }
            for (const key of modelRoles) {
                if (status?.rerun && key !== "chat") continue;
                if (rec[key][0]) defaults = selectRecommended(defaults, key, rec[key][0]);
            }
            setConfig({
                ...defaults,
                workspacePath: status?.workspacePath,
                workspaceDescription:
                    getConfigValue("experimental", "butler_main_workspace_description") || undefined,
                trustedWorkspacesRaw: getConfigValue("experimental", "butler_trusted_workspaces") || undefined,
                displayName: getConfigValue("experimental", "butler_display_name") || undefined,
            });
        }
        setPage("roles");
    }
    async function submitRoles() {
        if (editing || addingApi) {
            let c = config;
            if (path === "lie_flat") c = { ...config, butler: config.chat };
            const previous = status?.draft.config;
            if (
                previous &&
                roleKey(c.chat) === roleKey(previous.chat) &&
                roleKey(c.butler) === roleKey(previous.butler)
            ) {
                const changes: Partial<Pick<Config, "light" | "review" | "clearLight" | "clearReview" | "notes">> = {};
                const probe = probeSession(deadline(), setProgress);
                const nextNotes = { ...notes };
                const failed: Partial<Pick<Config, "light" | "review">> = {};
                for (const key of ["light", "review"] as const) {
                    const chosen = c[key];
                    const clear = key === "light" ? "clearLight" : "clearReview";
                    if (!chosen && c[clear]) {
                        changes[clear] = true;
                        delete nextNotes[key];
                        continue;
                    }
                    if (!chosen || roleKey(chosen) === roleKey(previous[key])) continue;
                    try {
                        changes[key] = chosen.verified ? chosen : await probe(chosen);
                        nextNotes[key] = { reason: c.notes?.[key]?.reason ?? "手动选择" };
                        changes.notes = { ...changes.notes, [key]: nextNotes[key] };
                    } catch (e) {
                        nextNotes[key] = { reason: c.notes?.[key]?.reason ?? "手动选择", error: explain(e) };
                        failed[key] = { ...chosen, verified: false };
                    }
                }
                const updated = Object.keys(changes).length
                    ? await updateRoles(changes)
                    : status!;
                setStatus(updated);
                const saved = updated.draft.config ?? previous;
                const savedNotes = { ...nextNotes, ...saved.notes };
                for (const key of ["light", "review"] as const) {
                    if (failed[key]) savedNotes[key] = nextNotes[key];
                    if (changes[key === "light" ? "clearLight" : "clearReview"]) delete savedNotes[key];
                }
                setConfig({ ...saved, ...failed, notes: savedNotes });
                setNotes(savedNotes);
                setEditing(false);
                setAddingApi(false);
                setPage("done");
                return;
            }
            await accept(await checkSelected(c, providers, installed, setProgress, previous));
            return;
        }
        setConfig((c) => ({ ...c, butler: c.butler ?? c.chat }));
        setPage("home");
    }
    async function retryRole(key: "light" | "review") {
        const value = config[key];
        if (!value) return;
        const probe = probeSession(deadline(), setProgress);
        try {
            const verified = await probe(value);
            const update: Partial<Config> = { [key]: verified };
            if (roleKey(config.light) === roleKey(config.review)) {
                update.light = verified;
                update.review = verified;
            }
            for (const k of ["light", "review"] as const) {
                const reason = config.notes?.[k]?.reason;
                if (update[k] && reason) update.notes = { ...update.notes, [k]: { reason } };
            }
            const next = await updateRoles(update);
            setStatus(next);
            setConfig(next.draft.config ?? { ...config, ...update });
            setNotes((prev) => {
                const updated = { ...prev };
                for (const k of ["light", "review"] as const) {
                    if (update[k]) updated[k] = { reason: next.draft.config?.notes?.[k]?.reason ?? prev[k]?.reason };
                }
                return updated;
            });
            await refresh();
        } catch (e) {
            setNotes((prev) => ({ ...prev, [key]: { error: explain(e) } }));
        }
    }
    const stageTitle =
        page === "sources" && (path === "expert" || addingApi)
            ? "添加 AI 来源"
            : page === "done" && path === "expert"
              ? "配置完成"
              : titles[page];
    const steps: Page[] =
        path === "expert"
            ? ["welcome", "sources", "roles", "home", "extras", "done"]
            : ["welcome", "sources", "done"];
    const startHome = () => invoke<void>("finish_onboarding");
    const effectiveRoles = [
        ["light", "轻量模型", status?.activeLight],
        ["review", "审核模型", status?.activeReview],
    ] as const;
    const failedRoles = effectiveRoles.filter(([key]) => config[key] && !config[key].verified);
    const sharedFailedAux =
        failedRoles.length === 2 && roleKey(config.light) === roleKey(config.review);
    const extraReady = (id: SetupId) => extraChecks[id] === true;
    return (
        <div
            data-aipp-window="onboarding"
            className="flex h-dvh min-h-0 flex-col bg-background text-foreground"
            onKeyDown={(e) => {
                const target = e.target as HTMLElement;
                if (
                    e.key !== "Enter" ||
                    e.nativeEvent.isComposing ||
                    e.defaultPrevented ||
                    target.closest("textarea,button,select,summary,[role=dialog],form") ||
                    embedded
                )
                    return;
                const button = e.currentTarget.querySelector<HTMLButtonElement>("footer [data-primary]");
                if (button && !button.disabled) {
                    e.preventDefault();
                    button.click();
                }
            }}
        >
            <WindowTopBar title="" bare maximizable={false} />
            {page !== "welcome" && (
                <header
                    key={`header-${page}`}
                    className="aipp-onboarding-enter mx-auto flex w-full max-w-3xl shrink-0 items-center justify-between px-8 pb-2 pt-1"
                >
                    <div>
                        <h1 className="text-2xl font-semibold tracking-tight">{stageTitle}</h1>
                        {page === "sources" && !embedded && (
                            <p className="mt-0.5 text-xs text-muted-foreground">
                                {path === "expert" || addingApi
                                    ? "添加一个或多个模型来源"
                                    : "选一个最方便的 AI 来源，以后随时能加"}
                            </p>
                        )}
                        {page === "extras" && (
                            <p className="mt-0.5 text-xs text-muted-foreground">
                                后台静默准备，不阻塞使用
                            </p>
                        )}
                    </div>
                    <div className="flex items-center gap-1.5" aria-label={stageTitle}>
                        {steps.map((s) => (
                            <span
                                key={s}
                                className={
                                    s === page
                                        ? "h-1.5 w-5 rounded-full bg-foreground"
                                        : "h-1.5 w-1.5 rounded-full bg-muted-foreground/25"
                                }
                            />
                        ))}
                    </div>
                </header>
            )}
            <main className={`min-h-0 flex-1 overflow-y-auto ${page === "welcome" ? "flex flex-col justify-center" : ""}`}>
                <div
                    key={page}
                    className="aipp-onboarding-enter mx-auto w-full max-w-3xl space-y-4 px-8 py-3"
                >
                    {!preset || !status ? (
                        <div className="flex items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
                            {!error && <Loader2 className="h-4 w-4 animate-spin" />}
                            <p>{error ? "无法载入引导配置" : "正在读取引导配置…"}</p>
                        </div>
                    ) : (
                        <>
                            {page === "welcome" && (
                                <div className="space-y-8">
                                    <div className="text-center">
                                        <h1
                                            style={{ "--stagger-delay": "0ms" } as CSSProperties}
                                            className="aipp-onboarding-stagger text-[32px] font-semibold tracking-tight text-foreground"
                                        >
                                            欢迎使用 AIPP
                                        </h1>
                                        <p
                                            style={{ "--stagger-delay": "70ms" } as CSSProperties}
                                            className="aipp-onboarding-stagger mt-2 text-sm text-muted-foreground"
                                        >
                                            选一种开始方式，之后都能在设置里改
                                        </p>
                                    </div>
                                    <div className="grid grid-cols-2 gap-5">
                                        <PathCard
                                            path="lie_flat"
                                            delayMs={130}
                                            onChoose={() => void run(() => choose("lie_flat"))}
                                        />
                                        <PathCard
                                            path="expert"
                                            delayMs={200}
                                            onChoose={() => void run(() => choose("expert"))}
                                        />
                                    </div>
                                </div>
                            )}
                            {page === "sources" && (
                                <SourceForm
                                    key={sourceRevision}
                                    path={addingApi ? "expert" : path}
                                    preset={preset}
                                    providers={providers}
                                    usable={usable}
                                    detection={detection}
                                    proxy={!!getConfigValue("network_config", "network_proxy").trim()}
                                    recharge={
                                        status.step === "awaiting_recharge"
                                            ? status.draft.rechargeProviderId
                                            : undefined
                                    }
                                    onChecked={accept}
                                    busy={busy}
                                    run={run}
                                    refresh={refresh}
                                    progress={setProgress}
                                    onNext={beginRoles}
                                    onControls={setEmbedded}
                                    footer={sourceButton}
                                />
                            )}
                            {page === "roles" && (
                                <ModelRoles
                                    config={config}
                                    activeLight={status?.activeLight}
                                    activeReview={status?.activeReview}
                                    setConfig={setConfig}
                                    models={models}
                                    providers={providers}
                                    ask={ask}
                                    onRecommend={(keys, followLight) => void run(async () => {
                                        const ranked = rankedRoles(providers, models, preset);
                                        const available = keys.filter(key => ranked[key].length);
                                        if (!available.length) {
                                            setError("没有匹配的推荐模型，请手动选择。");
                                            return;
                                        }
                                        const probe = probeSession(deadline(), setProgress);
                                        const result = await verifyRecommendedRoles(
                                            config, ranked, available, followLight, probe
                                        );
                                        setConfig(result.config);
                                        setError(result.errors.join("；"));
                                    })}
                                />
                            )}
                            {page === "home" && (
                                <HomeFields
                                    locked={status.workspaceLocked}
                                    config={config}
                                    setConfig={setConfig}
                                    models={models}
                                    providers={providers}
                                />
                            )}
                            {page === "extras" && (
                                <div className="divide-y divide-border/40 rounded-2xl bg-muted/60 dark:bg-muted/35">
                                    {allExtras.map((id) => {
                                        const ready = extraReady(id);
                                        const checked = ready || config.extras.includes(id);
                                        return (
                                            <label
                                                key={id}
                                                className={`flex items-center gap-3.5 px-5 py-3.5 transition-colors ${
                                                    ready ? "cursor-default opacity-85" : "cursor-pointer hover:bg-muted/30"
                                                }`}
                                            >
                                                <input
                                                    type="checkbox"
                                                    className="peer sr-only"
                                                    checked={checked}
                                                    disabled={ready}
                                                    onChange={(e) =>
                                                        setConfig({
                                                            ...config,
                                                            extras: e.target.checked
                                                                ? [...config.extras, id]
                                                                : config.extras.filter((v) => v !== id),
                                                        })
                                                    }
                                                />
                                                <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border border-border bg-background transition-colors peer-checked:border-foreground peer-checked:bg-foreground peer-checked:text-background peer-disabled:opacity-60 peer-focus-visible:ring-2 peer-focus-visible:ring-ring">
                                                    {checked && <Check className="h-2.5 w-2.5" strokeWidth={3} />}
                                                </span>
                                                <span className="flex h-8 w-8 shrink-0 items-center justify-center rounded-xl bg-background/80">
                                                    {extraIcons[id]}
                                                </span>
                                                <span className="min-w-0 flex-1">
                                                    <b className="block text-sm font-medium text-foreground">{setupNames[id]}</b>
                                                    <span className="block text-xs text-muted-foreground">
                                                        {extraInfo[id]}
                                                    </span>
                                                </span>
                                                {ready && (
                                                    <span className="inline-flex shrink-0 items-center gap-1 rounded-full bg-emerald-500/10 px-2.5 py-0.5 text-xs font-medium text-emerald-700 dark:text-emerald-400">
                                                        <CheckCircle2 className="h-3.5 w-3.5" />
                                                        ✓ 已就绪
                                                    </span>
                                                )}
                                                {extraChecks[id] === undefined && (
                                                    <span className="inline-flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                                                        <Loader2 className="h-3 w-3 animate-spin" />
                                                        正在检测…
                                                    </span>
                                                )}
                                                {typeof extraChecks[id] === "string" && (
                                                    <span className="max-w-[45%] break-words text-right text-xs text-amber-700 dark:text-amber-400">
                                                        {extraChecks[id]}
                                                    </span>
                                                )}
                                            </label>
                                        );
                                    })}
                                    {Object.values(extraChecks).some(value => typeof value === "string") && (
                                        <div className="flex justify-end px-4 py-2">
                                            <Button
                                                type="button"
                                                size="sm"
                                                variant="ghost"
                                                className="h-7 gap-1.5 text-xs"
                                                onClick={() => setExtraCheckAttempt(value => value + 1)}
                                            >
                                                <RefreshCw className="h-3 w-3" />
                                                重新检测
                                            </Button>
                                        </div>
                                    )}
                                </div>
                            )}
                            {page === "applying" && (
                                <div className="flex items-center gap-2.5 rounded-2xl bg-muted/30 p-5 text-sm text-muted-foreground">
                                    <Loader2 className="h-4 w-4 animate-spin" />
                                    <p>正在恢复上次已确认的选择。</p>
                                </div>
                            )}
                            {page === "done" && (
                                <div className="space-y-3.5">
                                    <div className="divide-y divide-border/40 rounded-2xl bg-muted/60 dark:bg-muted/35">
                                        <div className="flex items-start gap-4 p-5">
                                            <span className="shrink-0">
                                                <AnimatedAvatar animated size={56} appearance={defaultAppearance} />
                                            </span>
                                            <div className="min-w-0 flex-1 space-y-2.5 text-sm">
                                                <div>
                                                    <div className="flex flex-wrap items-center gap-2">
                                                        <span className="rounded-full bg-foreground/10 px-2 py-0.5 text-[11px] font-medium">
                                                            主力
                                                        </span>
                                                        <span className="font-medium text-foreground">
                                                            {label(roleKey(config.chat))}
                                                        </span>
                                                    </div>

                                                </div>
                                                <div className="grid grid-cols-2 gap-2">
                                                    {effectiveRoles.map(([, name, active]) => (
                                                        <div key={name} className="min-w-0 rounded-xl bg-background/75 px-3 py-2">
                                                            <span className="block text-[11px] text-muted-foreground">{name}</span>
                                                            <span className="block truncate text-xs font-medium text-foreground">
                                                                {label(active)}
                                                            </span>
                                                        </div>
                                                    ))}
                                                </div>
                                            </div>
                                            <Button
                                                variant="ghost"
                                                size="sm"
                                                className="h-8 shrink-0 rounded-full bg-background/80 px-3.5 text-xs hover:bg-background"
                                                onClick={() => {
                                                    setEditing(true);
                                                    setPage("roles");
                                                }}
                                            >
                                                换一个
                                            </Button>
                                        </div>
                                        {config.home === "butler_experiment" && (
                                            <div className="flex items-center justify-between gap-3 px-5 py-3 text-sm">
                                                <div className="flex min-w-0 items-center gap-2">
                                                    <FolderOpen className="h-4 w-4 shrink-0 text-muted-foreground" />
                                                    <span className="shrink-0 text-xs text-muted-foreground">工作区</span>
                                                    <span className="truncate font-mono text-xs text-foreground">
                                                        {status.workspacePath}
                                                    </span>
                                                </div>
                                                <Button
                                                    variant="ghost"
                                                    size="sm"
                                                    className="h-7 shrink-0 rounded-full px-3 text-xs text-muted-foreground hover:text-foreground"
                                                    onClick={() => void run(() => openPath(status.workspacePath))}
                                                >
                                                    打开文件夹
                                                </Button>
                                            </div>
                                        )}
                                        {searchStatus && (
                                            <div className="flex items-center gap-2 px-5 py-3 text-sm">
                                                <Globe className="h-4 w-4 shrink-0 text-muted-foreground" />
                                                <span className="text-xs text-muted-foreground">联网搜索</span>
                                                <span className="text-xs font-medium text-foreground">{searchStatus}</span>
                                            </div>
                                        )}
                                        {local(
                                            providers.find((p) => p.id === config.chat.providerId)?.api_type ?? ""
                                        ) &&
                                            !status.activeLight && (
                                                <div className="flex items-center justify-between gap-3 px-5 py-3 text-xs text-muted-foreground">
                                                    <span>标题取你的第一句话；补充 API 可自动起标题与减少工具确认</span>
                                                    <Button
                                                        variant="ghost"
                                                        size="sm"
                                                        className="h-7 shrink-0 rounded-full bg-background/80 px-3 text-xs hover:bg-background"
                                                        onClick={() => {
                                                            setAddingApi(true);
                                                            setPage("sources");
                                                        }}
                                                    >
                                                        加一个 API
                                                    </Button>
                                                </div>
                                            )}
                                    </div>
                                    {sharedFailedAux ? (
                                        <div className="rounded-2xl bg-amber-500/8 p-4 text-xs ring-1 ring-amber-500/25">
                                            <div className="flex items-start justify-between gap-3">
                                                <div className="flex items-start gap-2.5">
                                                    <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                                                    <div className="space-y-1">
                                                        <p className="text-sm font-medium text-foreground">
                                                            轻量与审核模型验证未通过 · {notes.light?.error || notes.review?.error || "未完成试调用"}
                                                        </p>
                                                        <p className="text-muted-foreground">
                                                            候选模型 · {label(roleKey(config.light))}
                                                        </p>

                                                        <p className="text-muted-foreground">
                                                            当前生效 · 轻量：{label(status.activeLight)} / 审核：{label(status.activeReview)}
                                                        </p>
                                                        {!status.activeLight && <p className="text-muted-foreground">对话总结暂停，群聊可用 @ 指定成员。</p>}
                                                        {!status.activeReview && <p className="text-muted-foreground">需要审核的操作会请你确认。</p>}
                                                    </div>
                                                </div>
                                                <Button
                                                    size="sm"
                                                    variant="ghost"
                                                    className="h-8 shrink-0 gap-1.5 rounded-full bg-background/80 px-3.5 text-xs hover:bg-background"
                                                    onClick={() => void run(() => retryRole("light"))}
                                                >
                                                    <RefreshCw className="h-3.5 w-3.5" />
                                                    重试轻量与审核
                                                </Button>
                                            </div>
                                        </div>
                                    ) : (
                                        failedRoles.map(([key, name, active]) => (
                                            <div
                                                className="flex items-start justify-between gap-3 rounded-2xl bg-amber-500/8 p-4 text-xs ring-1 ring-amber-500/25"
                                                key={key}
                                            >
                                                <div className="flex items-start gap-2.5">
                                                    <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-amber-600 dark:text-amber-400" />
                                                    <div className="space-y-1">
                                                        <p className="text-sm font-medium text-foreground">
                                                            {name}验证未通过 · {notes[key]?.error || "未完成试调用"}
                                                        </p>
                                                        <p className="text-muted-foreground">
                                                            候选模型 · {label(roleKey(config[key]))} · 当前生效 · {label(active)}
                                                        </p>

                                                        {!active && (
                                                            <p className="text-muted-foreground">
                                                                {key === "review"
                                                                    ? "需要审核的操作会请你确认。"
                                                                    : "对话总结暂停，群聊可用 @ 指定成员。"}
                                                            </p>
                                                        )}
                                                    </div>
                                                </div>
                                                <Button
                                                    size="sm"
                                                    variant="ghost"
                                                    className="h-8 shrink-0 gap-1.5 rounded-full bg-background/80 px-3.5 text-xs hover:bg-background"
                                                    onClick={() => void run(() => retryRole(key))}
                                                >
                                                    <RefreshCw className="h-3.5 w-3.5" />
                                                    重试{name}
                                                </Button>
                                            </div>
                                        ))
                                    )}
                                    {!status.activeReview && !config.review && (
                                        <p className="text-xs text-muted-foreground">需要审核的操作会请你确认。</p>
                                    )}
                                    {items.length > 0 && (
                                        <section className="space-y-2.5 rounded-2xl bg-muted/60 p-4 dark:bg-muted/35">
                                            <div className="flex items-center justify-between gap-2">
                                                <h3 className="text-sm font-medium">正在后台准备</h3>
                                                <span className="text-xs text-muted-foreground">
                                                    后台继续，不影响使用
                                                </span>
                                            </div>
                                            <SetupList items={items} />
                                        </section>
                                    )}
                                </div>
                            )}
                        </>
                    )}
                    {waiting && (
                        <div
                            role="status"
                            className="flex flex-wrap items-center justify-between gap-3 rounded-2xl bg-muted/35 px-4 py-3 text-sm"
                        >
                            <div className="flex min-w-0 items-center gap-2.5">
                                {!syncMessage && <Loader2 className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" />}
                                <span className="text-foreground">{syncMessage || "正在等待同步完成……"}</span>
                            </div>
                            <div className="flex shrink-0 items-center gap-1.5">
                                <Button
                                    type="button"
                                    size="sm"
                                    variant="ghost"
                                    className="h-7 rounded-full bg-background/80 px-3 text-xs hover:bg-background"
                                    onClick={() => void openSettings("data-sync-config")}
                                >
                                    打开同步设置
                                </Button>
                                <Button
                                    type="button"
                                    size="sm"
                                    variant="ghost"
                                    className="h-7 rounded-full px-2.5 text-xs text-muted-foreground"
                                    onClick={() => {
                                        setWaiting(false);
                                        setSyncMessage("");
                                    }}
                                >
                                    取消等待
                                </Button>
                            </div>
                        </div>
                    )}
                    {error && (
                        <div
                            role="alert"
                            className="flex flex-wrap items-center justify-between gap-2 rounded-2xl bg-destructive/8 px-4 py-3 text-sm ring-1 ring-destructive/20"
                        >
                            <div className="flex min-w-0 items-start gap-2">
                                <AlertCircle className="mt-0.5 h-4 w-4 shrink-0 text-destructive" />
                                <span>{error}</span>
                            </div>
                            <div className="flex shrink-0 items-center gap-1.5">
                                {loginId && (
                                    <Button
                                        variant="ghost"
                                        size="sm"
                                        className="h-7 rounded-full bg-background/80 px-3 text-xs"
                                        onClick={() =>
                                            void run(async () => {
                                                const provider = providers.find((p) => p.id === loginId);
                                                if (provider) {
                                                    await loginAgent(provider);
                                                    await accept(
                                                        await checkSelected(
                                                            config,
                                                            providers,
                                                            installed,
                                                            setProgress,
                                                            status?.draft.config
                                                        )
                                                    );
                                                }
                                            })
                                        }
                                    >
                                        登录 {providers.find((p) => p.id === loginId)?.name}
                                    </Button>
                                )}
                                {(!status || !preset) && (
                                    <Button variant="ghost" size="sm" className="h-7 rounded-full bg-background/80 px-3 text-xs" onClick={() => setBootRetry((n) => n + 1)}>
                                        重新读取
                                    </Button>
                                )}
                            </div>
                        </div>
                    )}
                </div>
            </main>
            <footer className="shrink-0 bg-background">
                <div className="mx-auto max-w-3xl px-8 py-4">
                    {busy && progress && (
                        <p role="status" className="mb-2 flex items-center gap-2 text-xs text-muted-foreground">
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                            {progress}
                        </p>
                    )}
                    <div className="flex items-center gap-2">
                        {page !== "done" && (
                            <Button
                                variant="ghost"
                                size="sm"
                                className="rounded-full text-xs text-muted-foreground hover:text-foreground"
                                disabled={busy}
                                onClick={() =>
                                    void run(async () => {
                                        if (editing || addingApi) {
                                            if (status?.draft.config) setConfig(status.draft.config);
                                            setEditing(false);
                                            setAddingApi(false);
                                            setPage("done");
                                            return;
                                        }
                                        await invoke("skip_onboarding");
                                    })
                                }
                            >
                                {editing || addingApi ? "取消" : "稍后再说"}
                            </Button>
                        )}
                        {(page === "welcome" || (page === "sources" && !embedded && !addingApi)) && (
                            <Button
                                variant="ghost"
                                size="sm"
                                className={`gap-1.5 rounded-full text-xs text-muted-foreground hover:text-foreground ${
                                    page === "welcome" ? "ml-auto" : ""
                                }`}
                                disabled={busy}
                                onClick={() =>
                                    void run(async () => {
                                        setWaiting(true);
                                        await openSettings("data-sync-config");
                                    })
                                }
                            >
                                <Cloud className="h-3.5 w-3.5" />
                                在别的电脑上用过 AIPP？登录同步
                            </Button>
                        )}
                        {page !== "welcome" && (
                        <div className="ml-auto flex items-center gap-2">
                            {["roles", "home", "extras"].includes(page) && !editing && (
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    className="rounded-full text-xs text-muted-foreground hover:text-foreground"
                                    disabled={busy}
                                    onClick={() =>
                                        setPage(page === "roles" ? "sources" : page === "home" ? "roles" : "home")
                                    }
                                >
                                    <ChevronLeft className="h-3.5 w-3.5" />
                                    上一步
                                </Button>
                            )}
                            {page === "sources" && !embedded && !addingApi && (
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    className="rounded-full text-xs text-muted-foreground hover:text-foreground"
                                    disabled={busy}
                                    onClick={() => {
                                        setSourceRevision((n) => n + 1);
                                        setPage("welcome");
                                    }}
                                >
                                    返回
                                </Button>
                            )}
                            <div ref={setSourceButton} />
                            {page === "roles" && (
                                <Button
                                    data-primary
                                    className="rounded-full px-6"
                                    disabled={busy || !config.chat.providerId}
                                    onClick={() => void run(submitRoles)}
                                >
                                    {editing || addingApi ? "应用" : "下一步"}
                                </Button>
                            )}
                            {page === "home" && (
                                <Button
                                    data-primary
                                    className="rounded-full px-6"
                                    disabled={
                                        busy ||
                                        (config.home === "butler_experiment" && !config.workspacePath?.trim())
                                    }
                                    onClick={() => setPage("extras")}
                                >
                                    下一步
                                </Button>
                            )}
                            {page === "extras" && (
                                <Button
                                    data-primary
                                    className="rounded-full px-6"
                                    disabled={busy}
                                    onClick={() =>
                                        void run(
                                            async () =>
                                                await accept(
                                                    await checkSelected(
                                                        {
                                                            ...config,
                                                            extras: [
                                                                ...new Set([
                                                                    ...config.extras,
                                                                    ...allExtras.filter(extraReady),
                                                                ]),
                                                            ],
                                                        },
                                                        providers,
                                                        installed,
                                                        setProgress,
                                                        undefined,
                                                        rankedRoles(providers, models, preset!)
                                                    )
                                                )
                                        )
                                    }
                                >
                                    完成
                                </Button>
                            )}
                            {page === "applying" && (
                                <Button
                                    data-primary
                                    className="rounded-full px-6"
                                    disabled={busy}
                                    onClick={() => void run(() => apply({ config, notes }))}
                                >
                                    重试应用
                                </Button>
                            )}
                            {page === "done" && (
                                <Button
                                    data-primary
                                    className="rounded-full px-7"
                                    onClick={() => void startHome().catch((e) => setError(detail(e)))}
                                >
                                    {path === "lie_flat"
                                        ? "开始使用"
                                        : config.home === "chat_ui"
                                          ? "打开聊天"
                                          : "打开管家"}
                                </Button>
                            )}
                        </div>
                        )}
                    </div>
                </div>
            </footer>
            <Dialog open={!!pending} onOpenChange={(v) => !v && setPending(undefined)}>
                <CompactDialogContent>
                    <CompactDialogHeader
                        title={pending?.kind === "channel" ? "切换总管家的大脑？" : "应用这些设置？"}
                    />
                    <CompactDialogBody>
                        {pending?.kind === "channel" ? (
                            "切换总管家的大脑会开始一段新的主会话，之前的对话可以在历史里查看。"
                        ) : (
                            <p>
                                将修改默认首页、新对话默认助手
                                {pending?.checked.config.home === "butler_experiment"
                                    ? "、总管家模型和工作区"
                                    : ""}
                                {pending?.checked.config.light?.verified ? "、轻量模型" : ""}
                                {pending?.checked.config.review?.verified ? "、审核模型" : ""}
                                。首页、助手和模型设置会同步到你的其他设备。
                            </p>
                        )}
                    </CompactDialogBody>
                    <CompactDialogFooter>
                        <Button variant="outline" onClick={() => setPending(undefined)}>
                            取消
                        </Button>
                        <Button
                            onClick={() => {
                                if (!pending) return;
                                const p = pending;
                                setPending(undefined);
                                void run(() => apply(p.checked, p.kind === "channel"));
                            }}
                        >
                            确认应用
                        </Button>
                    </CompactDialogFooter>
                </CompactDialogContent>
            </Dialog>
        </div>
    );
}
