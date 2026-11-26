import { useState } from "react";
import { createPortal } from "react-dom";
import { Sparkles } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { role } from "./onboardingService";
import { modelRoles, selectRecommended } from "./roleRecommendations";
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
    const [selected, setSelected] = useState(() => ({
        chat: agent ? source.config.chat.code : "",
        light: "",
        review: provider.api_type === "deepseek" && codes.includes("deepseek-flash") ? "deepseek-flash" : "",
    }));
    const [search, setSearch] = useState("");
    const [checked, setChecked] = useState<Checked>();
    const providers = agent ? [agent, provider] : [provider];
    const matches = codes.filter(code => code.toLowerCase().includes(search.trim().toLowerCase()));

    async function recommend() {
        let config = source.config;
        if (!agent && !ranked.chat.length) throw new Error("没有匹配的推荐模型，请手动选择。");
        for (const key of modelRoles) {
            if (key === "chat" && agent) continue;
            if (ranked[key][0]) config = selectRecommended(config, key, ranked[key][0]);
        }
        const result = await checkSelected(config, providers, () => true, progress, source.config, ranked);
        setChecked(result);
        setSearch("");
        setSelected({
            chat: result.config.chat.code, light: result.config.light?.code ?? "", review: result.config.review?.code ?? "",
        });
    }

    async function apply() {
        const previous = checked?.config;
        const unchanged = previous && modelRoles.every(key => selected[key] === (previous[key]?.code ?? ""));
        const result = checked && unchanged ? checked : await checkSelected({
            ...source.config,
            chat: agent ? source.config.chat : role(provider.id, selected.chat),
            light: selected.light ? role(provider.id, selected.light) : null,
            review: selected.review ? role(provider.id, selected.review) : null,
        }, providers, () => true, progress, previous ?? source.config);
        await onChecked({ ...result, config: { ...result.config, butler: result.config.chat } });
    }

    return (
        <fieldset disabled={busy} className="space-y-4 rounded-2xl bg-muted/45 p-5">
            <div className="flex items-center justify-between gap-2">
                <span className="font-medium">{provider.name}</span>
                <Button type="button" variant="outline" size="sm" disabled={!codes.length} onClick={() => void run(recommend)}>
                    <Sparkles className="h-4 w-4" />帮我选
                </Button>
            </div>
            {agent ? <p className="text-sm">主力模型 · {agent.name}</p> : (
                <div className="space-y-2">
                    <label htmlFor="onboarding-main-model" className="text-sm font-medium">主力模型</label>
                    <Input aria-label="搜索模型" placeholder="搜索模型…" value={search} onChange={e => setSearch(e.target.value)} />
                    <select id="onboarding-main-model" size={6} value={selected.chat}
                        className="w-full rounded-xl border border-input bg-background p-1 text-sm"
                        onChange={e => setSelected({ ...selected, chat: e.target.value })}>
                        <option value="" hidden disabled>请选择</option>
                        {matches.map(code =>
                            <option key={code} value={code} className="rounded px-2 py-1">{code}</option>
                        )}
                    </select>
                    {search && !matches.length && <p className="text-xs text-muted-foreground">没有匹配的模型</p>}
                    {search && selected.chat && <p className="text-xs text-muted-foreground">已选：{selected.chat}</p>}
                </div>
            )}
            {!codes.length && <p className="text-sm text-muted-foreground">该来源没有可用的聊天模型。</p>}
            <div className="grid gap-3 sm:grid-cols-2">
                {(["light", "review"] as const).map(key => (
                    <label key={key} className="grid gap-1.5 text-sm">
                        {key === "light" ? "轻量模型" : "审核模型"}
                        <select className="h-9 w-full min-w-0 rounded-xl border border-input bg-background px-2"
                            value={selected[key]} onChange={e => setSelected({ ...selected, [key]: e.target.value })}>
                            <option value="">不配置</option>
                            {codes.map(code => <option key={code} value={code}>{code}</option>)}
                        </select>
                        {checked?.notes[key]?.error && selected[key] === checked.config[key]?.code &&
                            <span className="text-xs text-amber-700">{checked.notes[key]?.error}</span>}
                    </label>
                ))}
            </div>
            {footer && createPortal(
                <Button data-primary type="button" className="rounded-full px-6" disabled={busy || !selected.chat}
                    onClick={() => void run(apply)}>应用</Button>, footer
            )}
        </fieldset>
    );
}
