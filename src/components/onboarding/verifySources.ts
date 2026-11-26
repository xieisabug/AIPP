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
    ProtocolChoiceError,
    role,
    type Config,
    type Provider,
    type Role,
    type Within,
} from "./onboardingService";

export interface RoleNote {
    reason?: string;
    error?: string;
    chooseProtocol?: boolean;
}
export type Notes = Partial<Record<"chat" | "light" | "review", RoleNote>>;
export interface Checked {
    config: Config;
    notes: Notes;
}
export interface ModelSource {
    provider: Provider;
    agent?: Provider;
    agentCodes?: string[];
    apiError?: string;
    config: Config;
    codes: string[];
    ranked: RankedRoles;
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
export async function prepareSource(
    provider: Provider,
    preset: OnboardingPreset,
    within: Within,
    progress: Progress,
    options: {
        agent?: Provider;
        models?: ModelSelectionResponse["available_models"];
    } = {}
): Promise<Checked | ModelSource> {
    const agent = options.agent ?? (local(provider.api_type) ? provider : undefined);
    let agentCodes: string[] = [];
    const notes: Notes = {};
    const config: Config = {
        chat: role(provider.id, ""),
        home: "butler_experiment",
        extras: ["skills", "office", "bun", "uv"],
    };
    if (agent) {
        progress(`正在读取 ${agent.name} 的模型列表…`);
        agentCodes = await checkAgent(agent);
        config.chat = {
            ...role(
                agent.id,
                ["acp", "codex_app_server", "claude_sdk"].includes(agent.api_type) ? "acp" : "__agent_default__"
            ),
            verified: true,
        };
        notes.chat = { reason: "本机已安装智能体" };
        if (provider.id === agent.id) return {
            provider, agent, agentCodes, codes: [], config: { ...config, notes, butler: config.chat },
            ranked: { chat: [], light: [], review: [] },
        };
    }
    progress("正在获取模型列表…");
    const apiDeadline = agent ? deadline() : within;
    let info: Awaited<ReturnType<typeof preview>>;
    try {
        info = await apiDeadline(() => preview(provider, preset, options.models));
    } catch (error) {
        if (!agent) throw error;
        return {
            provider, agent, agentCodes, codes: [], config: { ...config, notes, butler: config.chat },
            ranked: { chat: [], light: [], review: [] }, apiError: explain(error, provider.name),
        };
    }
    if (!agent) {
        progress("正在检查账户余额…");
        await checkBalance(provider, within);
    }
    const deepSeek = provider.api_type === "deepseek";
    if (deepSeek) config.models = [{ providerId: provider.id, codes: [...new Set(info.availableCodes)] }];
    const candidates = (rows: Candidate[]) => rows.map(({code, reason}) => ({
        model: role(provider.id, code), reason,
    }));
    return {
        provider, agent, agentCodes, config, codes: info.codes,
        ranked: { chat: candidates(info.picks.main), light: candidates(info.picks.light), review: candidates(info.picks.review) },
    };
}
// The stored verified flag marks a role as eligible to apply. In lie-flat mode the
// provider's model list supplies that evidence without generating any text.
export function catalogRoleChecker(available: Role[]) {
    const keys = new Set(available.map(roleKey));
    return (selected: Role): Role => {
        if (!keys.has(roleKey(selected))) throw new Error(`模型列表中没有 ${selected.code}，请重新选择模型`);
        return { ...selected, verified: true };
    };
}

export function lieFlatSelection(config: Config, check: (model: Role) => Role): Checked {
    const chat = check(config.chat);
    const result = { ...config, chat, butler: chat };
    const notes: Notes = {};
    for (const key of ["chat", "light", "review"] as const) {
        const selected = config[key];
        if (!selected) continue;
        if (key !== "chat") result[key] = check(selected);
        notes[key] = { reason: config.notes?.[key]?.reason ?? "手动选择" };
    }
    return { config: result, notes };
}

export async function checkSelected(
    config: Config,
    providers: Provider[],
    progress: Progress,
    applied?: Config | null,
    ranked?: RankedRoles
): Promise<Checked> {
    const within = deadline();
    const probe = probeSession(within, progress, providers);
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
        const result = await probe(r);
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
            notes[key] = { reason: config.notes?.[key]?.reason ?? "手动选择", error: explain(e) || detail(e),
                chooseProtocol: e instanceof ProtocolChoiceError || undefined };
            result[key] = { ...(e instanceof ProtocolChoiceError ? e.model : selected), verified: false };
        }
    }
    return { config: result, notes };
}
