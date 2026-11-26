import { useState } from "react";
import { createPortal } from "react-dom";
import { Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { role, selectAuxiliaryRole, type Config } from "./onboardingService";
import { modelRoles, selectRecommendedRoles } from "./roleRecommendations";
import { checkSelected, type Checked, type ModelSource, type Progress } from "./verifySources";

interface Props {
    source: ModelSource;
    footer: HTMLDivElement | null;
    busy: boolean;
    run: (fn: () => Promise<void>) => Promise<void>;
    progress: Progress;
    onChecked: (result: Checked) => Promise<void>;
}

export function LieFlatModels({ source, footer, busy, run, progress, onChecked }: Props) {
    const { provider, agent, codes, ranked } = source;
    const [config, setConfig] = useState<Config>(() => ({
        ...source.config,
        review: provider.api_type === "deepseek" && codes.includes("deepseek-flash")
            ? role(provider.id, "deepseek-flash") : source.config.review,
    }));

    function recommend() {
        const keys = modelRoles.filter(key => key !== "chat" || !agent);
        setConfig(current => selectRecommendedRoles(current, ranked, keys));
    }

    async function apply() {
        const providers = agent ? [agent, provider] : [provider];
        const result = await checkSelected(config, providers, () => true, progress, source.config, ranked);
        await onChecked({ ...result, config: { ...result.config, butler: result.config.chat } });
    }

    return (
        <fieldset disabled={busy} className="space-y-4 rounded-2xl bg-muted/45 p-5">
            <div className="flex items-center justify-between gap-2">
                <span className="font-medium">{provider.name}</span>
                <Button type="button" variant="outline" size="sm" disabled={!modelRoles.some(key => ranked[key].length)} onClick={recommend}>
                    <Sparkles className="h-4 w-4" />帮我选
                </Button>
            </div>
            {agent ? <p className="text-sm">主力模型 · {agent.name}</p> : (
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
            {!codes.length && <p className="text-sm text-muted-foreground">该来源没有可用的聊天模型。</p>}
            <div className="grid gap-3 sm:grid-cols-2">
                {(["light", "review"] as const).map(key => (
                    <label key={key} className="grid gap-1.5 text-sm">
                        {key === "light" ? "轻量模型" : "审核模型"}
                        <select className="h-9 w-full min-w-0 rounded-xl border border-input bg-background px-2"
                            value={config[key]?.code ?? ""} onChange={e => setConfig(selectAuxiliaryRole(
                                config, key, e.target.value ? role(provider.id, e.target.value) : null
                            ))}>
                            <option value="">不配置</option>
                            {codes.map(code => <option key={code} value={code}>{code}</option>)}
                        </select>
                    </label>
                ))}
            </div>
            {footer && createPortal(
                <Button data-primary type="button" className="rounded-full px-6" disabled={busy || !config.chat.code}
                    onClick={() => void run(apply)}>应用</Button>, footer
            )}
        </fieldset>
    );
}
