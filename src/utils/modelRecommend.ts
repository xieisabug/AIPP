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
    main: {groups: {agent: 20, chat: 3},
        inputShare: 0.80, qualityWeight: 0.90, costScale: 2},
    light: {groups: {factual: 1},
        inputShare: 0.95, qualityWeight: 0.40, costScale: 0.35},
    review: {groups: {factual: 1},
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

function officialOffers(catalog?: RecommendationCatalog) {
    const official = new Map<string, Offer[]>();
    for (const [id, provider] of Object.entries(catalog?.providers ?? {})) {
        for (const offer of Object.values(provider.models)) {
            if (!offer.model?.startsWith(id + "/")) continue;
            const entries = official.get(offer.model) ?? [];
            entries.push(offer);
            official.set(offer.model, entries);
        }
    }
    return official;
}

function offersForSource(source: Source, catalog: RecommendationCatalog | undefined, official: Map<string, Offer[]>): Item[] {
    const endpoint = endpointKey(source.endpoint);
    const locations = Object.entries(catalog?.providers ?? {}).map(([id, provider]) => ({
        id, provider, base: Boolean(endpoint && provider.endpoints.some(url => endpointKey(url) === endpoint)),
    }));
    return [...new Set(source.codes)].map(code => {
        const origins = new Set<string>();
        const native: Offer[] = [];
        const hits = locations.filter(({id, provider, base}) => {
            if (!Object.hasOwn(provider.models, code)) return false;
            const offer = provider.models[code];
            if (offer.model) origins.add(offer.model);
            if (offer.model?.startsWith(id + "/")) native.push(offer);
            return base || Boolean(endpoint && endpointKey(offer.endpoint) === endpoint);
        });
        const exact = hits.length === 1 ? hits[0].provider.models[code] : undefined;
        const direct = native.length === 1 ? native[0] : undefined;
        let model = exact?.model ?? direct?.model;
        if (!model && catalog && (Object.hasOwn(catalog.identities ?? {}, code) || Object.hasOwn(catalog.benchmarks, code)))
            model = code;
        if (!model && origins.size === 1) model = [...origins][0];
        if (!model && origins.size > 1) {
            // A rolling gateway alias may identify several releases. Only use a unique
            // lab-listed release when every possible identity belongs to the same family.
            const ids = [...origins];
            const family = catalog?.identities?.[ids[0]]?.family;
            const listed = ids.filter(id => official.has(id));
            if (family && listed.length === 1 && ids.every(id => id.split("/")[0] === listed[0].split("/")[0] &&
                catalog?.identities?.[id]?.family === family)) model = listed[0];
        }
        const references = official.get(model ?? "") ?? [];
        const matching = direct?.model === model ? direct : undefined;
        const lab = model?.split("/")[0];
        const canonical = model && lab ? catalog?.providers[lab]?.models[model.slice(lab.length + 1)] : undefined;
        const reference = matching ?? (canonical?.model === model ? canonical : undefined) ?? (references.every(row => row.input === references[0].input &&
            row.output === references[0].output) ? references[0] : undefined);
        const pricing = hasPrice(exact) ? exact : reference;
        const offer = {...(exact ?? reference), input: pricing?.input, output: pricing?.output};
        return {providerId: source.id, code, model, offer};
    }).filter(item => item.offer?.chat !== false);
}

interface Observation { group: string; percentile: number }
function hasPrice(offer?: Offer): offer is Offer & { input: number; output: number } {
    return typeof offer?.input === "number" && Number.isFinite(offer.input) && offer.input >= 0 &&
        typeof offer.output === "number" && Number.isFinite(offer.output) && offer.output >= 0;
}

// Compare against the full board at the same effort. Count each test once,
// using the highest comparable published effort when several exist.
const efforts = ["minimal", "low", "medium", "high", "xhigh", "max"];
function calibratedEvidence(catalog: RecommendationCatalog | undefined, wanted: Set<string>) {
    const result = new Map<string, Observation[]>();
    const covered = new Set<string>();
    if (!catalog) return {models: result, covered};
    const boards = new Map<string, Map<string, Benchmark>>();
    for (const data of [catalog.benchmarks, catalog.independent_benchmarks ?? {}]) {
        for (const [id, rows] of Object.entries(data)) {
            for (const row of rows) {
                if (!row.test.startsWith("arena:") || !Number.isFinite(row.score)) continue;
                const key = JSON.stringify([row.test, row.effort ?? ""]);
                const board = boards.get(key) ?? new Map<string, Benchmark>();
                board.set(id, row);
                boards.set(key, board);
            }
        }
    }
    const used = new Map<string, Set<string>>();
    const ordered = [...boards.values()].sort((a, b) =>
        efforts.indexOf(b.values().next().value!.effort ?? "") - efforts.indexOf(a.values().next().value!.effort ?? ""));
    for (const board of ordered) {
        if (board.size < 2) continue;
        const {group, test} = board.values().next().value!;
        covered.add(group);
        const seen = used.get(test) ?? new Set<string>();
        used.set(test, seen);
        for (const [id, row] of board) {
            if (!wanted.has(id) || seen.has(id)) continue;
            seen.add(id);
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
        const price = item.offer;
        const cost = hasPrice(price) ? price.input * policy.inputShare + price.output * (1 - policy.inputShare) : Infinity;
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
    const catalog = preset.recommendation_catalog;
    const official = officialOffers(catalog);
    const items = sources.flatMap(source => offersForSource(source, catalog, official));
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
