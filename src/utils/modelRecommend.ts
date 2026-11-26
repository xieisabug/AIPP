export interface RankingEntry {
    base: string;
    aliases?: string[];
}
interface Offer { model?: string; input?: number; output?: number; tools?: boolean; chat?: boolean; endpoint?: string }
interface Benchmark {
    group: string; test: string; score: number;
    low?: number; high?: number; tolerance?: number; lowerBetter?: boolean; effort?: string;
}
interface Identity { family?: string; date?: string; effort?: string }
interface RecommendationCatalog {
    benchmarks: Record<string, Benchmark[]>;
    independent_benchmarks?: Record<string, Benchmark[]>;
    identities?: Record<string, Identity>;
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

interface Policy {
    groups: Record<string, number>;
    inputShare: number;
    qualityWeight: number;
    costScale: number; // USD per million mixed tokens at which affordability is 0.5
}
const policies: Record<Role, Policy> = {
    main: {groups: {general: 60, reasoning: 12, agent: 20, instruction: 5, chat: 3},
        inputShare: 0.80, qualityWeight: 0.90, costScale: 2},
    light: {groups: {general: 20, instruction: 40, context: 20, factual: 20},
        inputShare: 0.95, qualityWeight: 0.40, costScale: 0.35},
    review: {groups: {unsafeAllowance: 45, safety: 20, general: 15, reasoning: 5, instruction: 5, factual: 10},
        inputShare: 0.90, qualityWeight: 0.80, costScale: 1},
};
function endpointKey(raw?: string) {
    if (!raw) return "";
    try {
        const url = new URL(raw);
        if (url.search || url.hash || url.username || url.password) return "";
        return url.origin + url.pathname.replace(/\/+$/, "");
    } catch { return ""; }
}
interface Item { providerId: number; code: string; model?: string; offer?: Offer; }
type Ranked = Candidate & { providerId: number; score: number; quality: number; cost: number; };

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
        if (!model && catalog && (Object.hasOwn(catalog.benchmarks, code) || Object.hasOwn(catalog.identities ?? {}, code))) model = code;
        if (!model && origins.size === 1) model = origins.values().next().value;
        return {providerId: source.id, code, model, offer};
    }).filter(item => item.offer?.chat !== false);
}

interface Observation { group: string; percentile: number }
const indexPrefix = "aa:intelligence:";

// Normalize each test against the full snapshot, independent of the user's candidate list.
function calibratedEvidence(catalog: RecommendationCatalog | undefined, wanted: Set<string>) {
    const result = new Map<string, Observation[]>();
    const covered = new Set<string>();
    if (!catalog) return {models: result, covered};
    const boards = new Map<string, Map<string, Benchmark>>();
    let latestIndex = "";
    const versions = new Intl.Collator("en", {numeric: true});
    const ids = new Set([...Object.keys(catalog.benchmarks), ...Object.keys(catalog.independent_benchmarks ?? {})]);
    for (const id of ids) {
        for (const row of [...(catalog.benchmarks[id] ?? []), ...(catalog.independent_benchmarks?.[id] ?? [])]) {
            if (!Number.isFinite(row.score)) continue;
            // The composite index measures the same tasks and units across efforts.
            // Give each model one reference observation at its deployed default.
            const composite = row.test.startsWith(indexPrefix);
            if (composite && row.effort && row.effort !== catalog.identities?.[id]?.effort) continue;
            const key = JSON.stringify([row.group, row.test, composite ? "" : row.effort ?? "", !!row.lowerBetter]);
            const board = boards.get(key) ?? new Map<string, Benchmark>();
            // If both records exist for a composite, prefer the verified default effort.
            if (!composite || row.effort || !board.get(id)?.effort) board.set(id, row);
            boards.set(key, board);
        }
    }
    for (const board of boards.values()) {
        const test = board.values().next().value!.test;
        if (board.size >= 2 && test.startsWith(indexPrefix) && versions.compare(test, latestIndex) > 0) latestIndex = test;
    }
    for (const board of boards.values()) {
        if (board.size < 2) continue;
        const first = board.values().next().value!;
        const composite = first.test.startsWith(indexPrefix);
        if (composite && first.test !== latestIndex) continue;
        const group = composite ? "general" : first.group;
        covered.add(group);
        for (const [id, row] of board) {
            if (!wanted.has(id) || (row.effort && row.effort !== catalog.identities?.[id]?.effort)) continue;
            let wins = 0;
            for (const [otherId, other] of board) {
                if (otherId === id) continue;
                let delta = (row.score - other.score) * (row.lowerBetter ? -1 : 1);
                if (row.low != null && row.high != null && other.low != null && other.high != null) {
                    if (row.low <= other.high && other.low <= row.high) delta = 0;
                } else if (Math.abs(delta) <= (row.tolerance ?? 2)) delta = 0;
                wins += (1 + Math.sign(delta)) / 2;
            }
            // Two neutral pseudo-observations temper the extremes on small boards.
            const percentile = (wins + 1) / (board.size + 1);
            const observations = result.get(id) ?? [];
            observations.push({group, percentile});
            result.set(id, observations);
        }
    }
    return {models: result, covered};
}

function roleQuality(observations: Observation[], policy: Policy, covered: Set<string>) {
    const domains = new Map<string, {sum: number; count: number}>();
    for (const {group, percentile} of observations) {
        if (!policy.groups[group]) continue;
        const domain = domains.get(group) ?? {sum: 0, count: 0};
        domain.sum += percentile;
        domain.count++;
        domains.set(group, domain);
    }
    if (!domains.size) return 0;
    // Partial evidence earns limited credit for uncovered domains.
    let quality = 0;
    let totalWeight = 0;
    for (const [group, weight] of Object.entries(policy.groups)) {
        if (!covered.has(group)) continue;
        totalWeight += weight;
        const domain = domains.get(group);
        quality += weight * (domain ? domain.sum / domain.count : 0.35);
    }
    return quality / totalWeight;
}

function rankRole(items: Item[], role: Role, catalog: RecommendationCatalog | undefined,
    evidence: ReturnType<typeof calibratedEvidence>): Ranked[] {
    const policy = policies[role];
    const eligible = items.filter(item => role !== "main" || item.offer?.tools !== false);
    const qualityByModel = new Map<string, number>();
    const rows = eligible.map(item => {
        const {input, output} = item.offer ?? {};
        const priced = typeof input === "number" && Number.isFinite(input) && input >= 0 &&
            typeof output === "number" && Number.isFinite(output) && output >= 0;
        const cost = priced ? input * policy.inputShare + output * (1 - policy.inputShare) : Infinity;
        const id = item.model ?? "";
        const identity = catalog?.identities?.[id];
        const quality = qualityByModel.get(id) ?? roleQuality(evidence.models.get(id) ?? [], policy, evidence.covered);
        qualityByModel.set(id, quality);
        const affordability = 1 / (1 + cost / policy.costScale);
        const score = policy.qualityWeight * quality ** 2 + (1 - policy.qualityWeight) * affordability;
        return {providerId: item.providerId, code: item.code, reason: "自动选择",
            quality, score, cost, family: identity?.family ? id.split("/")[0] + "/" + identity.family : id || item.code,
            date: identity?.date ?? ""};
    });
    return rows.sort((a, b) => b.score - a.score || a.cost - b.cost || a.family.localeCompare(b.family) ||
        b.date.localeCompare(a.date) || a.code.localeCompare(b.code) || a.providerId - b.providerId);
}

/** All roles compare the same set of available offers across provider instances. */
export function rankModels(sources: Source[], preset: OnboardingPreset) {
    const items = sources.flatMap(source => offersForSource(source, preset));
    const evidence = calibratedEvidence(preset.recommendation_catalog, new Set(items.map(item => item.model ?? "")));
    return { main: rankRole(items, "main", preset.recommendation_catalog, evidence),
        light: rankRole(items, "light", preset.recommendation_catalog, evidence),
        review: rankRole(items, "review", preset.recommendation_catalog, evidence) };
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
