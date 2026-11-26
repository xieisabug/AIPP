export interface RankingEntry {
    base: string;
    aliases?: string[];
}
interface Offer { model?: string; input?: number; output?: number; tools?: boolean; chat?: boolean; endpoint?: string }
interface Benchmark { group: string; test: string; score: number }
interface RecommendationCatalog {
    benchmarks: Record<string, Benchmark[]>;
    providers: Record<string, { endpoints: string[]; models: Record<string, Offer> }>;
}
export interface OnboardingPreset {
    recommendation_catalog?: RecommendationCatalog;
    recommended_provider: {
        name: string;
        api_type: string;
        endpoint: string;
        price_hint: string;
        key_guide: { title: string; url: string }[];
        models: { main: string; light: string; review?: string };
        extras?: { enable_deepseek_search?: boolean };
    };
    model_ranking: { main: RankingEntry[]; light: RankingEntry[]; review?: RankingEntry[] };
    strip_suffixes: string[];
    light_keywords: string[];
}
export interface Candidate {
    code: string;
    reason: string;
}

interface Source { id: number; api_type: string; endpoint?: string; codes: string[] }
type Role = "main" | "light" | "review";

const policies: Record<Role, { weights: Record<string, number>; inputShare: number; priceWeight: number }> = {
    main: { weights: { reasoning: 4, agent: 4, instruction: 2 }, inputShare: 0.8, priceWeight: 0.2 },
    light: { weights: { instruction: 3, context: 2, factual: 1 }, inputShare: 0.95, priceWeight: 1 },
    review: { weights: { safety: 5, factual: 2, instruction: 1 }, inputShare: 0.9, priceWeight: 0.15 },
};
function endpointKey(raw?: string) {
    if (!raw) return "";
    try {
        const url = new URL(raw);
        if (url.search || url.hash || url.username || url.password) return "";
        return url.origin + url.pathname.replace(/\/+$/, "");
    } catch { return ""; }
}
interface Item { providerId: number; code: string; model?: string; offer?: Offer; preferred: boolean }
type Ranked = Candidate & { providerId: number; value: number; cost: number; preferred: boolean };

function offersForSource(source: Source, preset: OnboardingPreset): Item[] {
    const catalog = preset.recommendation_catalog;
    const endpoint = endpointKey(source.endpoint);
    const locations = Object.entries(catalog?.providers ?? {}).map(([id, provider]) => ({
        id, provider, base: Boolean(endpoint && provider.endpoints.some(url => endpointKey(url) === endpoint)),
    }));
    return [...new Set(source.codes)].map(code => {
        const origins = new Set<string>();
        const hits = locations.filter(({provider, base}) => {
            if (!Object.hasOwn(provider.models, code)) return false;
            const offer = provider.models[code];
            if (offer.model) origins.add(offer.model);
            return base || Boolean(endpoint && endpointKey(offer.endpoint) === endpoint);
        });
        // Identical model codes on different gateways can identify the underlying model,
        // but only a unique endpoint+code match identifies the price this user pays.
        const hit = hits.length === 1 ? hits[0] : undefined;
        const offer = hit?.provider.models[code];
        let model = offer?.model;
        if (!model && catalog && Object.hasOwn(catalog.benchmarks, code)) model = code;
        if (!model && origins.size === 1) model = origins.values().next().value;
        return {providerId: source.id, code, model, offer,
            preferred: hit?.id === preset.recommended_provider.api_type && code === preset.recommended_provider.models.main};
    }).filter(item => item.offer?.chat !== false);
}

function rankRole(items: Item[], role: Role, catalog?: RecommendationCatalog): Ranked[] {
    const policy = policies[role];
    const eligible = items.filter(item => role !== "main" || item.offer?.tools !== false);
    const models = new Set(eligible.map(it => it.model).filter((id): id is string => !!id));
    const owners = new Map<string, number>();
    for (const model of models) {
        for (const test of new Set((catalog?.benchmarks[model] ?? []).map(b => b.test)))
            owners.set(test, (owners.get(test) ?? 0) + 1);
    }
    const testsByGroup = new Map<string, Set<string>>();
    const totals = new Map<string, Map<string, number>>();
    for (const model of models) {
        const byGroup = new Map<string, number>();
        for (const b of catalog?.benchmarks[model] ?? []) {
            if (!policy.weights[b.group] || (owners.get(b.test) ?? 0) < 2) continue;
            const tests = testsByGroup.get(b.group) ?? new Set<string>();
            tests.add(b.test);
            testsByGroup.set(b.group, tests);
            byGroup.set(b.group, (byGroup.get(b.group) ?? 0) + b.score);
        }
        totals.set(model, byGroup);
    }
    // Every model uses the same denominator. Missing results contribute zero, and a
    // heavily benchmarked domain cannot drown out a smaller domain with the same weight.
    const scores = new Map<string, number>();
    for (const [model, byGroup] of totals) {
        let sum = 0;
        let totalWeight = 0;
        for (const [group, tests] of testsByGroup) {
            sum += policy.weights[group] * (byGroup.get(group) ?? 0) / tests.size;
            totalWeight += policy.weights[group];
        }
        scores.set(model, totalWeight ? sum / totalWeight : 0);
    }
    return eligible.map(item => {
        const {input, output} = item.offer ?? {};
        const priced = typeof input === "number" && Number.isFinite(input) && input >= 0 &&
            typeof output === "number" && Number.isFinite(output) && output >= 0;
        const cost = priced ? input * policy.inputShare + output * (1 - policy.inputShare) : Infinity;
        const quality = scores.get(item.model ?? "") ?? 0;
        const value = priced ? quality / (1 + policy.priceWeight * Math.log1p(cost)) : quality;
        return { providerId: item.providerId, code: item.code, reason: "自动选择", value, cost, preferred: item.preferred };
    }).sort((a, b) => Number(b.value > 0) - Number(a.value > 0) ||
        Number(Number.isFinite(b.cost)) - Number(Number.isFinite(a.cost)) ||
        b.value - a.value || a.cost - b.cost || Number(b.preferred) - Number(a.preferred) ||
        a.code.localeCompare(b.code) || a.providerId - b.providerId);
}

/** All roles compare the same set of available offers across provider instances. */
export function rankModels(sources: Source[], preset: OnboardingPreset) {
    const items = sources.flatMap(source => offersForSource(source, preset));
    return { main: rankRole(items, "main", preset.recommendation_catalog),
        light: rankRole(items, "light", preset.recommendation_catalog),
        review: rankRole(items, "review", preset.recommendation_catalog) };
}
export function recommendModels(codes: string[], apiType: string, preset: OnboardingPreset, endpoint?: string) {
    return rankModels([{ id: 0, api_type: apiType, codes, endpoint }], preset);
}

export function recommendedCodes(picks: ReturnType<typeof recommendModels>, selected: string[] = []) {
    return [
        ...new Set([
            ...selected,
            ...[picks.main, picks.light, picks.review].flatMap((items) => items.map((item) => item.code)),
        ]),
    ].slice(0, 10);
}
