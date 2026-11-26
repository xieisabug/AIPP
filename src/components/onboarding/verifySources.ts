import { invoke } from "@tauri-apps/api/core";
import type { ModelSelectionResponse } from "@/components/config/llmModelTypes";
import { type Candidate, type OnboardingPreset } from "@/utils/modelRecommend";
import { firstVerified, type ModelRole, type RankedRoles } from "./roleRecommendations";
import {
    roleKey,
    checkAgent,
    deadline,
    detail,
    explain,
    local,
    preview,
    probeSession,
    role,
    type Config,
    type Provider,
    type Role,
    type Within,
} from "./onboardingService";

export interface RoleNote {
    reason?: string;
    error?: string;
}
export type Notes = Partial<Record<"chat" | "light" | "review", RoleNote>>;
export interface Checked {
    config: Config;
    notes: Notes;
}
export interface ManualModels {
    provider: Provider;
    codes: string[];
    light: string;
}
export type Progress = (text: string) => void;
export async function checkBalance(provider: Provider, within: Within) {
    if (provider.api_type !== "deepseek") return;
    const balance = await within(() =>
        invoke<{ available: boolean; total: string }>("get_deepseek_balance", { llmProviderId: provider.id })
    );
    if (!balance.available || Number(balance.total) <= 0)
        throw { kind: "insufficient_balance", message: "DeepSeek 余额为 0" };
}
export async function autoCheck(
    provider: Provider,
    preset: OnboardingPreset,
    within: Within,
    progress: Progress,
    options: {
        agent?: Provider;
        manual?: { main: string; light: string };
        checkMainBalance?: boolean;
        models?: ModelSelectionResponse["available_models"];
    } = {}
): Promise<Checked | ManualModels> {
    const agent = options.agent;
    const notes: Notes = {};
    const probe = probeSession(within, progress);
    const config: Config = {
        chat: role(provider.id, ""),
        home: "butler_experiment",
        extras: ["skills", "office", "bun", "uv"],
    };
    if (agent) {
        await within(() => checkAgent(agent));
        config.chat = {
            ...role(
                agent.id,
                ["acp", "codex_app_server", "claude_sdk"].includes(agent.api_type) ? "acp" : "__agent_default__"
            ),
            verified: true,
        };
        notes.chat = { reason: "本机已安装智能体" };
        if (provider.id === agent.id) return { config: { ...config, butler: config.chat }, notes };
    }
    let info: Awaited<ReturnType<typeof preview>>;
    try {
        progress("正在获取模型列表…");
        info = await within(() => preview(provider, preset, options.models));
        if (options.checkMainBalance && !agent) {
            progress("正在检查账户余额…");
            await checkBalance(provider, within);
        }
    } catch (e) {
        if (!agent) throw e;
        const defaults = preset.recommended_provider.models;
        for (const key of ["light", "review"] as const) {
            config[key] = role(provider.id, defaults[key] || defaults.light);
            notes[key] = { reason: "服务商推荐预设", error: explain(e, provider.name) };
        }
        return { config: { ...config, butler: config.chat }, notes };
    }
    const deepSeek = provider.api_type === "deepseek";
    if (deepSeek) config.models = [{ providerId: provider.id, codes: [...new Set(info.availableCodes)] }];
    const manual = options.manual;
    const manualLight = manual?.light ? [{ code: manual.light, reason: "手动选择" }] : [];
    const candidates = {
        chat: manual ? [{ code: manual.main, reason: "手动选择" }] : info.picks.main,
        light: manual ? manualLight : info.picks.light,
        review: manual ? manualLight : info.picks.review,
    };
    if (deepSeek && info.codes.includes("deepseek-flash")) {
        candidates.review = [
            { code: "deepseek-flash", reason: "自动选择" },
            ...candidates.review.filter(candidate => candidate.code !== "deepseek-flash"),
        ];
    }
    const fallback = { provider, codes: info.codes, light: info.picks.light[0]?.code ?? "" };
    if (!agent && !candidates.chat.length) return fallback;
    async function select(key: ModelRole, list: Candidate[]) {
        if (!list.length) return null;
        const candidates = list.map(candidate => ({ model: role(provider.id, candidate.code), reason: candidate.reason }));
        try {
            const selected = await firstVerified(candidates, probe);
            notes[key] = { reason: selected.reason };
            return selected.model;
        } catch (error) {
            if (key === "chat") throw error;
            notes[key] = { reason: candidates[0].reason, error: explain(error, provider.name) };
            return candidates[0].model;
        }
    }
    if (!agent) {
        try {
            config.chat = (await select("chat", candidates.chat))!;
        } catch (e) {
            if (!manual && e && typeof e === "object" && "kind" in e && e.kind === "model_not_found")
                return fallback;
            throw e;
        }
    }
    config.light = await select("light", candidates.light);
    config.review = await select("review", candidates.review);
    config.butler = config.chat;
    return { config, notes };
}
export async function checkSelected(
    config: Config,
    providers: Provider[],
    installed: (p: Provider) => boolean,
    progress: Progress,
    applied?: Config | null,
    ranked?: RankedRoles
): Promise<Checked> {
    const within = deadline();
    const probe = probeSession(within, progress);
    const notes: Notes = {};
    const verified = new Map<string, Role>();
    for (const previous of [applied?.chat, applied?.butler, applied?.light, applied?.review]) {
        if (previous?.verified) verified.set(JSON.stringify([previous.providerId, previous.code]), previous);
    }
    async function check(r: Role) {
        const key = JSON.stringify([r.providerId, r.code]);
        if (verified.has(key)) return verified.get(key)!;
        const p = providers.find((p) => p.id === r.providerId);
        if (!p) throw new Error("找不到所选模型提供商");
        let result: Role;
        if (local(p.api_type)) {
            if (!installed(p)) throw new Error(`${p.name} 尚未安装或缺少可用 Node.js`);
            progress(`正在检查 ${p.name}…`);
            await within(() => checkAgent(p));
            result = { ...r, verified: true };
        } else result = await probe(r);
        verified.set(key, result);
        return result;
    }
    async function checkRole(key: ModelRole, selected: Role) {
        const reason = config.notes?.[key]?.reason;
        const candidates = ranked?.[key] ?? [];
        // Only an automatic selection may advance to another candidate. A manual choice has no alternatives.
        const index = candidates.findIndex(item => roleKey(item.model) === roleKey(selected) && item.reason === reason);
        if (index >= 0) {
            const verified = await firstVerified(candidates.slice(index), check);
            notes[key] = { reason: verified.reason };
            return verified.model;
        }
        const verified = await check(selected);
        const isAgent = providers.some(p => p.id === selected.providerId && local(p.api_type));
        notes[key] = { reason: reason ?? (isAgent ? "本机已安装智能体" : "手动选择") };
        return verified;
    }
    const result = { ...config, chat: await checkRole("chat", config.chat) };
    if (config.home === "butler_experiment" && config.butler) {
        result.butler = config.butler === config.chat ? result.chat : await check(config.butler);
    }
    for (const key of ["light", "review"] as const) {
        const selected = config[key];
        if (!selected) continue;
        // Linked auxiliary roles share the final model even when the original light model returned 404.
        if (key === "review" && config.light && result.light &&
            roleKey(selected) === roleKey(config.light) &&
            config.notes?.review?.reason === config.notes?.light?.reason) {
            result.review = result.light;
            notes.review = notes.light;
            continue;
        }
        try {
            result[key] = await checkRole(key, selected);
        } catch (e) {
            notes[key] = { reason: config.notes?.[key]?.reason ?? "手动选择", error: explain(e) || detail(e) };
            result[key] = { ...selected, verified: false };
        }
    }
    return { config: result, notes };
}
