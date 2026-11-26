import type { ModelForSelect } from "@/hooks/useFilteredModels";
import { rankModels, type OnboardingPreset } from "@/utils/modelRecommend";
import {
    explain, local, role, selectAuxiliaryRole,
    type Config, type Provider, type Role,
} from "./onboardingService";

export const modelRoles = ["chat", "light", "review"] as const;
export type ModelRole = (typeof modelRoles)[number];
export interface RoleCandidate {
    model: Role;
    reason: string;
}
export type RankedRoles = Record<ModelRole, RoleCandidate[]>;
type Probe = (model: Role) => Promise<Role>;

export function rankedRoles(
    providers: Provider[], models: ModelForSelect[], preset: OnboardingPreset
): RankedRoles {
    const picks = rankModels(providers.filter(p => p.is_enabled && !local(p.api_type)).map(p => ({
        ...p, codes: models.filter(m => m.llm_provider_id === p.id).map(m => m.code),
    })), preset);
    const candidates = (rows: typeof picks.main) => rows.map(row => ({
        model: role(row.providerId, row.code), reason: row.reason,
    }));
    return {chat: candidates(picks.main), light: candidates(picks.light), review: candidates(picks.review)};
}

/** 首选之后仅在模型不存在时换候选，整次操作传入同一个带截止时间的 probe。 */
export async function firstVerified(candidates: RoleCandidate[], probe: Probe): Promise<RoleCandidate> {
    if (!candidates.length) throw new Error("没有匹配的推荐模型，请手动选择。");
    const lastAttempt = Math.min(candidates.length - 1, 3);
    for (let index = 0; ; index++) {
        const candidate = candidates[index];
        try {
            return { ...candidate, model: await probe(candidate.model) };
        } catch (error) {
            const missing = error && typeof error === "object" && "kind" in error && error.kind === "model_not_found";
            if (!missing || index === lastAttempt) throw error;
        }
    }
}

export function selectRecommended(config: Config, key: ModelRole, candidate: RoleCandidate): Config {
    const next = key === "chat"
        ? { ...config, chat: candidate.model }
        : selectAuxiliaryRole(config, key, candidate.model);
    return { ...next, notes: { ...next.notes, [key]: { reason: candidate.reason } } };
}

export async function verifyRecommendedRoles(
    config: Config,
    ranked: RankedRoles,
    keys: readonly ModelRole[],
    followLight: boolean,
    probe: Probe
) {
    let next = config;
    const errors: string[] = [];
    for (const key of keys) {
        if (key === "review" && followLight && keys.includes("light")) continue;
        try {
            const selected = await firstVerified(ranked[key], probe);
            next = selectRecommended(next, key, selected);
            if (key === "light" && followLight) next = selectRecommended(next, "review", selected);
        } catch (error) {
            errors.push(explain(error));
        }
    }
    return { config: next, errors: [...new Set(errors)] };
}
