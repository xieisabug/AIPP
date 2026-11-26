import { useState } from "react";
import { ModelTestField } from "./ModelTestField";
import { createPortal } from "react-dom";
import { Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { localLightWarning, roleKey, role, selectAuxiliaryRole, type Config } from "./onboardingService";
import { modelRoles, selectRecommendedRoles } from "./roleRecommendations";
import { catalogRoleChecker, lieFlatSelection, type Checked, type ModelSource } from "./verifySources";

interface Props {
    source: ModelSource;
    footer: HTMLDivElement | null;
    busy: boolean;
    run: (fn: () => Promise<void>) => Promise<void>;
    onChecked: (result: Checked) => Promise<void>;
}

export function LieFlatModels({ source, footer, busy, run, onChecked }: Props) {
    const { provider, agent, agentCodes = [], codes, ranked } = source;
    const [config, setConfig] = useState<Config>(source.config);
    const [separateReview, setSeparateReview] = useState(!!source.config.review &&
        roleKey(source.config.review) !== roleKey(source.config.light));

    function recommend() {
        const keys = modelRoles.filter(key => key !== "chat" || !agent);
        setConfig(current => selectRecommendedRoles(current, ranked, keys, !separateReview));
    }

    async function apply() {
        const available = codes.map(code => role(provider.id, code));
        if (agent) available.push(source.config.chat, ...agentCodes.map(code => role(agent.id, code)));
        await onChecked(lieFlatSelection(config, catalogRoleChecker(available)));
    }

    return (
        <fieldset disabled={busy} className="space-y-4 rounded-2xl bg-muted/45 p-5">
            <div className="flex items-center justify-between gap-2">
                <span className="font-medium">{provider.name}</span>
                <Button type="button" variant="outline" size="sm" disabled={!modelRoles.some(key => ranked[key].length)} onClick={recommend}>
                    <Sparkles className="h-4 w-4" />帮我选
                </Button>
            </div>
            <ModelTestField label="主力模型" selected={config.chat} provider={agent ?? provider}>
                {agent ? <p className="self-center text-sm">主力模型 · {agent.name}</p> : (
                    <label className="grid gap-1.5 text-sm">主力模型
                        <select className="h-9 w-full min-w-0 rounded-xl border border-input bg-background px-2"
                            value={config.chat.code} onChange={e => setConfig({
                                ...config, chat: role(provider.id, e.target.value),
                                notes: { ...config.notes, chat: { reason: "手动选择" } },
                            })}>
                            <option value="" disabled>请选择</option>
                            {codes.map(code => <option key={code} value={code}>{code}</option>)}
                        </select>
                    </label>
                )}
            </ModelTestField>
            {source.apiError && <p role="alert" className="text-sm text-destructive">{provider.name} 模型列表读取失败：{source.apiError}。仍可选择 {agent?.name} 的轻量模型。</p>}
            {!agent && !codes.length && <p className="text-sm text-muted-foreground">该来源没有可用的聊天模型。</p>}
            <div className="grid gap-3 sm:grid-cols-2">
                {(["light", "review"] as const).filter(key => key === "light" || codes.length > 0).map(key => (
                    <ModelTestField key={key} label={key === "light" ? "轻量模型" : "审核模型"} selected={config[key]}
                        provider={agent && config[key]?.providerId === agent.id ? agent : provider}>
                        <label className="grid gap-1.5 text-sm">
                            {key === "light" ? "轻量模型" : "审核模型"}
                            <select className="h-9 w-full min-w-0 rounded-xl border border-input bg-background px-2"
                                value={(agent && config[key]?.providerId === agent.id ? roleKey(config[key]) : config[key]?.code) ?? ""}
                                onChange={e => {
                                    const value = e.target.value;
                                    const selected = agent && agentCodes.find(code => roleKey(role(agent.id, code)) === value);
                                    const model = selected && agent ? role(agent.id, selected) : value ? role(provider.id, value) : null;
                                    const next = selectAuxiliaryRole(config, key, model);
                                    if (key === "review") setSeparateReview(roleKey(model) !== roleKey(config.light));
                                    if (key === "light" && !separateReview) {
                                        setConfig(selectAuxiliaryRole(next, "review", selected ? null : model));
                                    } else setConfig(next);
                                }}>
                                <option value="">不配置</option>
                                {codes.map(code => <option key={code} value={code}>{code}</option>)}
                                {agent && key === "light" && <optgroup label={agent.name}>
                                    {agentCodes.map(code => <option key={code} value={roleKey(role(agent.id, code))}>{code}</option>)}
                                </optgroup>}
                            </select>
                        </label>
                    </ModelTestField>
                ))}
            </div>
            {agent && config.light?.providerId === agent.id && <p role="status" className="text-xs text-amber-700 dark:text-amber-400">{localLightWarning}</p>}
            {footer && createPortal(
                <Button data-primary type="button" className="rounded-full px-6" disabled={busy || !config.chat.code}
                    onClick={() => void run(apply)}>应用</Button>, footer
            )}
        </fieldset>
    );
}
