import { useState } from "react";
import { Check, CheckCircle2, ChevronDown, ChevronRight, Sparkles, Zap } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import AnimatedAvatar from "@/components/avatar/AnimatedAvatar";
import { defaultAppearance } from "@/components/butler/butlerAppearance";
import { MainWorkspaceFields } from "@/components/butler/steps/StepWorkspaceConfig";
import {
    parseTrustedWorkspaces,
    serializeButlerWorkspaceConfig,
    type TrustedWorkspace,
} from "@/components/butler/butlerWorkspaceConfig";
import { FolderPicker } from "@/components/config/FolderPicker";
import type { ModelForSelect } from "@/hooks/useFilteredModels";
import { modelRoles, type ModelRole } from "./roleRecommendations";
import type { Config, Provider, Role } from "./onboardingService";
import { local, roleKey, roleOf, selectAuxiliaryRole } from "./onboardingService";

export function RoleSelect({
    label,
    value,
    models,
    providers,
    onChange,
    optional = false,
    keepExisting = false,
}: {
    label: string;
    value?: Role | null;
    models: ModelForSelect[];
    providers: Provider[];
    onChange: (r: Role | null) => void;
    optional?: boolean;
    keepExisting?: boolean;
}) {
    return (
        <label className="grid gap-1.5 text-sm font-medium">
            <span className="text-[13px] text-foreground/85">{label}</span>
            <div className="relative">
                <select
                    aria-label={label}
                    className="h-9 w-full appearance-none rounded-xl border border-border/70 bg-background/90 pl-3.5 pr-8 text-sm font-normal text-foreground transition-colors hover:border-foreground/25 focus-visible:border-ring focus-visible:outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
                    value={!value && keepExisting ? "keep" : roleKey(value)}
                    onChange={(e) => {
                        const m = models.find((m) => roleKey(roleOf(m)) === e.target.value);
                        onChange(m ? roleOf(m) : null);
                    }}
                >
                    {!value && keepExisting && <option value="keep" disabled>沿用已有配置</option>}
                    <option value="" disabled={!optional}>
                        {optional ? "不配置" : "请选择"}
                    </option>
                    {[false, true].map((isAgent) => {
                        const group = models.filter((m) =>
                            providers.some((p) => p.id === m.llm_provider_id && local(p.api_type) === isAgent)
                        );
                        return (
                            group.length > 0 && (
                                <optgroup key={String(isAgent)} label={isAgent ? "本机智能体" : "API 聊天模型"}>
                                    {group.map((m) => (
                                        <option value={roleKey(roleOf(m))} key={roleKey(roleOf(m))}>
                                            {providers.find((p) => p.id === m.llm_provider_id)?.name} /{" "}
                                            {m.id === 0 ? "默认模型" : m.code}
                                        </option>
                                    ))}
                                </optgroup>
                            )
                        );
                    })}
                </select>
                <ChevronDown
                    aria-hidden="true"
                    className="pointer-events-none absolute right-2.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted-foreground"
                />
            </div>
        </label>
    );
}
export function ModelRoles({
    config,
    setConfig,
    providers,
    models,
    ask,
    onRecommend,
    activeLight,
    activeReview,
}: {
    config: Config;
    setConfig: (c: Config) => void;
    providers: Provider[];
    models: ModelForSelect[];
    ask: string | null;
    onRecommend: (keys: ModelRole[], followLight: boolean) => void;
    activeLight?: string | null;
    activeReview?: string | null;
}) {
    const [separate, setSeparate] = useState(
        (roleKey(config.review) || (config.clearReview ? "" : activeReview) || "") !==
            (roleKey(config.light) || (config.clearLight ? "" : activeLight) || "")
    );
    const api = models.filter((m) => !local(providers.find((p) => p.id === m.llm_provider_id)?.api_type ?? ""));
    function suggest(key?: ModelRole) {
        const requested = key ? [key] : modelRoles;
        const nativeChat = providers.some(p => p.id === config.chat.providerId && local(p.api_type));
        if (!key) setSeparate(true);
        onRecommend(requested.filter(k => !(k === "chat" && nativeChat)), !!key && !separate);
    }
    const hasLight = Boolean(config.light || (!config.clearLight && activeLight));
    return (
        <div className="space-y-3">
            <div className="flex items-center justify-end">
                <Button
                    variant="ghost"
                    size="sm"
                    className="h-8 gap-1.5 rounded-full bg-muted/50 px-3.5 text-xs hover:bg-muted"
                    onClick={() => suggest()}
                >
                    <Sparkles className="h-3.5 w-3.5" />
                    全部帮我选
                </Button>
            </div>
            <div className="divide-y divide-border/40 rounded-2xl bg-muted/60 dark:bg-muted/35">
                <div className="space-y-2 px-5 py-3.5">
                    <div className="grid grid-cols-[1fr_auto] items-end gap-2">
                        <RoleSelect
                            label="新对话默认使用"
                            value={config.chat}
                            models={models}
                            providers={providers}
                            onChange={(r) => {
                                if (r) {
                                    const followChat = !config.butler || roleKey(config.butler) === roleKey(config.chat);
                                    setConfig({
                                        ...config,
                                        chat: r,
                                        butler: followChat ? r : config.butler,
                                        notes: { ...config.notes, chat: { reason: "手动选择" } },
                                    });
                                }
                            }}
                        />
                        <Button
                            variant="ghost"
                            size="sm"
                            className="h-9 gap-1 rounded-xl bg-background/80 px-3 text-xs hover:bg-background"
                            onClick={() => suggest("chat")}
                        >
                            <Sparkles className="h-3.5 w-3.5 text-muted-foreground" />
                            帮我选
                        </Button>
                    </div>

                    <div className="flex items-center gap-1.5 text-xs text-muted-foreground">
                        <Zap className="h-3 w-3 shrink-0 opacity-70" />
                        <span>Ask 弹窗 · 快速使用助手 · {ask || "未配置 API 模型"}</span>
                    </div>
                </div>
                <div className="space-y-2 px-5 py-3.5">
                    <div className="grid grid-cols-[1fr_auto] items-end gap-2">
                        <RoleSelect
                            label="轻量模型"
                            optional
                            value={config.light}
                            keepExisting={!!activeLight && !config.clearLight}
                            models={api}
                            providers={providers}
                            onChange={(r) => {
                                const next = selectAuxiliaryRole(config, "light", r);
                                setConfig(separate ? next : selectAuxiliaryRole(next, "review", r));
                            }}
                        />
                        <Button
                            variant="ghost"
                            size="sm"
                            className="h-9 gap-1 rounded-xl bg-background/80 px-3 text-xs hover:bg-background"
                            onClick={() => suggest("light")}
                        >
                            <Sparkles className="h-3.5 w-3.5 text-muted-foreground" />
                            帮我选
                        </Button>
                    </div>

                    <p className={`text-xs ${hasLight ? "text-muted-foreground" : "text-amber-700 dark:text-amber-400"}`}>
                        {hasLight
                            ? "用于标题、摘要、群聊调度与进度总结"
                            : "需要 API 模型。缺少轻量模型时，标题取第一句话，对话总结暂停，群聊可用 @ 指定成员。"}
                    </p>
                </div>
                <div className="space-y-2 px-5 py-3.5">
                    <label className="flex cursor-pointer items-center gap-2.5 text-sm select-none">
                        <input
                            type="checkbox"
                            className="peer sr-only"
                            checked={separate}
                            onChange={(e) => {
                                setSeparate(e.target.checked);
                                if (!e.target.checked) {
                                    let light = config.light ?? null;
                                    // On a rerun the light field may still be set to "沿用已有配置".
                                    if (!light && !config.clearLight && activeLight) {
                                        const split = activeLight.lastIndexOf("%%");
                                        const providerId = Number(activeLight.slice(split + 2));
                                        if (split > 0 && Number.isSafeInteger(providerId) && providerId > 0) {
                                            light = { providerId, code: activeLight.slice(0, split), verified: false };
                                        }
                                    }
                                    setConfig(selectAuxiliaryRole(config, "review", light));
                                }
                            }}
                        />
                        <span className="flex h-4 w-4 shrink-0 items-center justify-center rounded-full border border-border bg-background transition-colors peer-checked:border-foreground peer-checked:bg-foreground peer-checked:text-background peer-focus-visible:ring-2 peer-focus-visible:ring-ring">
                            {separate && <Check className="h-2.5 w-2.5" strokeWidth={3} />}
                        </span>
                        <span className="text-[13px] font-medium text-foreground">单独指定审核模型</span>
                        {!separate && (
                            <span className="text-xs text-muted-foreground">同轻量模型</span>
                        )}
                    </label>
                    {separate && (
                        <>
                            <div className="grid grid-cols-[1fr_auto] items-end gap-2 pt-1">
                                <RoleSelect
                                    label="审核模型"
                                    optional
                                    value={config.review}
                                    keepExisting={!!activeReview && !config.clearReview}
                                    models={api}
                                    providers={providers}
                                    onChange={(r) => setConfig(selectAuxiliaryRole(config, "review", r))}
                                />
                                <Button
                                    variant="ghost"
                                    size="sm"
                                    className="h-9 gap-1 rounded-xl bg-background/80 px-3 text-xs hover:bg-background"
                                    onClick={() => suggest("review")}
                                >
                                    <Sparkles className="h-3.5 w-3.5 text-muted-foreground" />
                                    帮我选
                                </Button>
                            </div>

                        </>
                    )}
                </div>
            </div>
        </div>
    );
}
function HomeOptionCard({
    home,
    selected,
    onSelect,
}: {
    home: "chat_ui" | "butler_experiment";
    selected: boolean;
    onSelect: () => void;
}) {
    const isButler = home === "butler_experiment";
    const [seq, setSeq] = useState(0);
    return (
        <button
            type="button"
            aria-label={isButler ? "管家" : "聊天"}
            aria-pressed={selected}
            onMouseEnter={() => setSeq((n) => n + 1)}
            onFocus={() => setSeq((n) => n + 1)}
            onClick={onSelect}
            className={`group relative flex h-44 flex-col items-center justify-center rounded-3xl px-5 text-center transition-all duration-300 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring ${
                selected
                    ? "bg-muted ring-2 ring-foreground/80 dark:bg-muted/65"
                    : "bg-muted/60 hover:bg-muted dark:bg-muted/35 dark:hover:bg-muted/55"
            }`}
        >
            {selected && (
                <CheckCircle2 className="absolute right-4 top-4 h-4 w-4 text-foreground" />
            )}
            <AnimatedAvatar
                animated
                size={76}
                appearance={{
                    ...defaultAppearance,
                    accessories: {
                        eyes: isButler ? null : "round-glasses",
                        head: isButler ? "crown" : null,
                    },
                }}
                actionRequest={seq ? { action: isButler ? "straighten-crown" : "glasses", sequence: seq } : undefined}
            />
            <strong className="mt-2.5 text-lg font-medium tracking-tight">
                {isButler ? "管家" : "聊天"}
            </strong>
            <div className="grid grid-rows-[0fr] opacity-0 transition-all duration-300 group-hover:grid-rows-[1fr] group-hover:opacity-100 group-focus-visible:grid-rows-[1fr] group-focus-visible:opacity-100">
                <span className="overflow-hidden pt-1 text-xs text-muted-foreground">
                    {isButler ? "多助手协作、工作区与定时任务" : "经典单对话窗口，轻量直接"}
                </span>
            </div>
        </button>
    );
}
export function HomeFields({
    config,
    setConfig,
    models,
    providers,
    locked,
}: {
    locked: boolean;
    config: Config;
    setConfig: (c: Config) => void;
    models: ModelForSelect[];
    providers: Provider[];
}) {
    const [trusted, setTrusted] = useState("");
    const workspaces = parseTrustedWorkspaces(config.trustedWorkspacesRaw);
    function updateTrusted(value: TrustedWorkspace[]) {
        setConfig({
            ...config,
            trustedWorkspacesRaw: serializeButlerWorkspaceConfig({
                mainWorkspacePath: config.workspacePath,
                trustedWorkspaces: value,
            }).trustedWorkspacesRaw,
        });
    }
    return (
        <div className="space-y-4">
            <div className="grid grid-cols-2 gap-4">
                {(["chat_ui", "butler_experiment"] as const).map((home) => (
                    <HomeOptionCard
                        key={home}
                        home={home}
                        selected={home === config.home}
                        onSelect={() => setConfig({ ...config, home, butler: config.butler || config.chat })}
                    />
                ))}
            </div>
            {config.home === "butler_experiment" && (
                <div className="aipp-onboarding-enter space-y-3.5 rounded-2xl bg-muted/60 p-5 dark:bg-muted/35">
                    <RoleSelect
                        label="总管家的大脑"
                        models={models}
                        providers={providers}
                        value={config.butler ?? config.chat}
                        onChange={(r) => setConfig({ ...config, butler: r })}
                    />
                    <MainWorkspaceFields
                        locked={locked}
                        mainWorkspacePath={config.workspacePath ?? ""}
                        mainWorkspaceDescription={config.workspaceDescription ?? ""}
                        onMainWorkspacePathChange={(workspacePath) => setConfig({ ...config, workspacePath })}
                        onMainWorkspaceDescriptionChange={(workspaceDescription) =>
                            setConfig({ ...config, workspaceDescription: workspaceDescription || undefined })
                        }
                    />
                    <details className="group rounded-xl bg-background/60 px-3.5 py-2.5">
                        <summary className="flex cursor-pointer list-none items-center gap-1.5 text-xs font-medium text-muted-foreground hover:text-foreground">
                            <ChevronRight className="h-3.5 w-3.5 transition-transform group-open:rotate-90" />
                            更多可信目录
                        </summary>
                        <div className="mt-2.5 space-y-2 pt-1">
                            {workspaces.map((p) => (
                                <div key={p.path} className="flex items-center justify-between gap-2 rounded-lg bg-muted/40 px-2.5 py-1.5 text-xs">
                                    <span className="min-w-0 truncate font-mono">{p.path}</span>
                                    <Button
                                        size="sm"
                                        variant="ghost"
                                        className="h-6 px-2 text-xs"
                                        onClick={() => updateTrusted(workspaces.filter((i) => i.path !== p.path))}
                                    >
                                        移除
                                    </Button>
                                </div>
                            ))}
                            <div className="flex items-center gap-2">
                                <div className="flex-1">
                                    <FolderPicker value={trusted} onChange={setTrusted} placeholder="选择额外可信目录" />
                                </div>
                                <Button
                                    size="sm"
                                    variant="secondary"
                                    disabled={!trusted.trim()}
                                    onClick={() => {
                                        updateTrusted([...workspaces, { path: trusted.trim(), description: "" }]);
                                        setTrusted("");
                                    }}
                                >
                                    添加
                                </Button>
                            </div>
                        </div>
                    </details>
                    <label className="grid gap-1.5 text-[13px] font-medium text-foreground/85">
                        <span>显示名称（可选）</span>
                        <Input
                            className="rounded-xl bg-background/90"
                            placeholder="总管家"
                            value={config.displayName ?? ""}
                            onChange={(e) => setConfig({ ...config, displayName: e.target.value })}
                        />
                    </label>
                    <p className="text-xs text-muted-foreground">飞书接入可稍后在设置中完成</p>
                </div>
            )}
        </div>
    );
}
