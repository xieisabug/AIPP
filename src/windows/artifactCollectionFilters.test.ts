import { describe, expect, it } from "vitest";
import {
    collectTags,
    collectTypes,
    filterAndSortCollections,
    parseTags,
    type CollectionFilter,
    type CollectionFilterItem,
} from "./artifactCollectionFilters";

function item(id: number, overrides: Partial<CollectionFilterItem> = {}): CollectionFilterItem {
    return {
        id,
        name: `工具${id}`,
        description: "",
        artifact_type: "react",
        tags: null,
        created_time: "2026-11-01 08:00:00",
        last_used_time: null,
        use_count: 0,
        ...overrides,
    };
}

const ALL: CollectionFilter = { query: "", type: null, tag: null, sort: "frequent" };

describe("parseTags", () => {
    it("should read comma separated tags with Chinese punctuation", () => {
        expect(parseTags("图表, 数据，可视化、图表")).toEqual(["图表", "数据", "可视化"]);
    });

    it("should read legacy JSON array tags", () => {
        expect(parseTags('["tag1", "tag2", 3]')).toEqual(["tag1", "tag2"]);
    });

    it("should return nothing for empty values", () => {
        expect(parseTags(null)).toEqual([]);
        expect(parseTags("  ")).toEqual([]);
    });
});

describe("collectTags and collectTypes", () => {
    it("should order by how many items use them", () => {
        const items = [
            item(1, { tags: "数据,图表", artifact_type: "html" }),
            item(2, { tags: "图表" }),
            item(3, { tags: "图表,工具" }),
        ];
        expect(collectTags(items)).toEqual(["图表", "工具", "数据"]);
        expect(collectTypes(items)).toEqual(["react", "html"]);
    });
});

describe("filterAndSortCollections", () => {
    const items = [
        item(1, { name: "记账本", tags: "财务", use_count: 2, last_used_time: "2026-11-20 10:00:00", created_time: "2026-10-01 00:00:00" }),
        item(2, { name: "番茄钟", artifact_type: "html", use_count: 5, last_used_time: "2026-11-10 10:00:00", created_time: "2026-11-02 00:00:00" }),
        item(3, { name: "汇率换算", description: "财务小工具", use_count: 0, created_time: "2026-11-25 00:00:00" }),
    ];

    it("should sort by use count when showing the most used first", () => {
        expect(filterAndSortCollections(items, ALL).map((i) => i.id)).toEqual([2, 1, 3]);
    });

    it("should sort by last use and put never-used items last", () => {
        expect(filterAndSortCollections(items, { ...ALL, sort: "recent" }).map((i) => i.id)).toEqual([1, 2, 3]);
    });

    it("should sort by collection time", () => {
        expect(filterAndSortCollections(items, { ...ALL, sort: "newest" }).map((i) => i.id)).toEqual([3, 2, 1]);
    });

    it("should combine type, tag and text filters", () => {
        expect(filterAndSortCollections(items, { ...ALL, type: "html" }).map((i) => i.id)).toEqual([2]);
        expect(filterAndSortCollections(items, { ...ALL, tag: "财务" }).map((i) => i.id)).toEqual([1]);
        expect(filterAndSortCollections(items, { ...ALL, query: "财务" }).map((i) => i.id)).toEqual([1, 3]);
        expect(filterAndSortCollections(items, { ...ALL, query: "财务", type: "html" })).toEqual([]);
    });
});
