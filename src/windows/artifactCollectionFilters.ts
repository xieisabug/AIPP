/** 小工具库（Artifacts 合集）的筛选与排序。 */

export type CollectionSort = "frequent" | "recent" | "newest";

export const COLLECTION_SORT_LABELS: Record<CollectionSort, string> = {
    frequent: "最常用",
    recent: "最近使用",
    newest: "最新收藏",
};

export interface CollectionFilterItem {
    id: number;
    name: string;
    description: string;
    artifact_type: string;
    tags?: string | null;
    created_time: string;
    last_used_time?: string | null;
    use_count: number;
}

export interface CollectionFilter {
    query: string;
    /** 类型；null 为全部 */
    type: string | null;
    /** 标签；null 为全部 */
    tag: string | null;
    sort: CollectionSort;
}

/** 标签字段可能是 JSON 数组（旧数据）或逗号分隔文本（编辑表单写入）。 */
export function parseTags(raw?: string | null): string[] {
    const text = raw?.trim();
    if (!text) return [];
    if (text.startsWith("[")) {
        try {
            const value: unknown = JSON.parse(text);
            if (Array.isArray(value)) {
                return dedupe(value.filter((tag): tag is string => typeof tag === "string"));
            }
        } catch {
            // 不是合法 JSON，按分隔文本处理
        }
    }
    return dedupe(text.split(/[,，、;；]/));
}

function dedupe(tags: string[]): string[] {
    return Array.from(new Set(tags.map((tag) => tag.trim()).filter(Boolean)));
}

/** 出现过的标签，按使用的收藏数从多到少排列。 */
export function collectTags(items: CollectionFilterItem[]): string[] {
    const counts = new Map<string, number>();
    for (const item of items) {
        for (const tag of parseTags(item.tags)) counts.set(tag, (counts.get(tag) ?? 0) + 1);
    }
    return Array.from(counts.entries())
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0], "zh-CN"))
        .map(([tag]) => tag);
}

/** 出现过的类型，按收藏数从多到少排列。 */
export function collectTypes(items: CollectionFilterItem[]): string[] {
    const counts = new Map<string, number>();
    for (const item of items) counts.set(item.artifact_type, (counts.get(item.artifact_type) ?? 0) + 1);
    return Array.from(counts.entries())
        .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
        .map(([type]) => type);
}

/** SQLite `CURRENT_TIMESTAMP` 是不带时区的 UTC 文本，按 UTC 解析后比较。 */
function timeValue(raw?: string | null): number {
    if (!raw) return 0;
    const iso = /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}:\d{2}$/.test(raw) ? `${raw.replace(" ", "T")}Z` : raw;
    const value = Date.parse(iso);
    return Number.isNaN(value) ? 0 : value;
}

export function filterAndSortCollections<T extends CollectionFilterItem>(items: T[], filter: CollectionFilter): T[] {
    const query = filter.query.trim().toLowerCase();
    const matched = items.filter((item) => {
        if (filter.type && item.artifact_type !== filter.type) return false;
        const tags = parseTags(item.tags);
        if (filter.tag && !tags.includes(filter.tag)) return false;
        if (!query) return true;
        return (
            item.name.toLowerCase().includes(query) ||
            item.description.toLowerCase().includes(query) ||
            tags.some((tag) => tag.toLowerCase().includes(query))
        );
    });
    const byNewest = (a: T, b: T) => timeValue(b.created_time) - timeValue(a.created_time) || b.id - a.id;
    const byRecent = (a: T, b: T) => timeValue(b.last_used_time) - timeValue(a.last_used_time);
    const compare: Record<CollectionSort, (a: T, b: T) => number> = {
        frequent: (a, b) => b.use_count - a.use_count || byRecent(a, b) || byNewest(a, b),
        recent: (a, b) => byRecent(a, b) || byNewest(a, b),
        newest: byNewest,
    };
    return [...matched].sort(compare[filter.sort]);
}
