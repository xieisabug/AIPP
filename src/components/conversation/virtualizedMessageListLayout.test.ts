import { describe, expect, it } from "vitest";

import {
    buildVirtualizedLayout,
    findVisibleRange,
    getCenteredScrollTopForIndex,
} from "./virtualizedMessageListLayout";

describe("virtualizedMessageListLayout", () => {
    it("builds stable row tops from estimated and measured heights", () => {
        const layout = buildVirtualizedLayout(
            [
                { key: "a", estimatedHeight: 100 },
                { key: "b", estimatedHeight: 120 },
                { key: "c", estimatedHeight: 80 },
            ],
            { b: 200 },
        );

        expect(layout.heights).toEqual([100, 200, 80]);
        expect(layout.tops).toEqual([0, 116, 332]);
        expect(layout.totalHeight).toBe(412);
    });

    it("finds an overscanned visible range", () => {
        const layout = buildVirtualizedLayout(
            Array.from({ length: 20 }, (_, index) => ({
                key: `row-${index}`,
                estimatedHeight: 100,
            })),
            {},
        );

        const range = findVisibleRange(layout, 700, 300, 100);
        expect(range.startIndex).toBe(5);
        expect(range.endIndex).toBeGreaterThan(range.startIndex);
        expect(range.endIndex).toBeLessThanOrEqual(20);
    });

    it("centers a target row while clamping to bounds", () => {
        const layout = buildVirtualizedLayout(
            [
                { key: "a", estimatedHeight: 100 },
                { key: "b", estimatedHeight: 100 },
                { key: "c", estimatedHeight: 100 },
                { key: "d", estimatedHeight: 100 },
            ],
            {},
        );

        expect(getCenteredScrollTopForIndex(layout, 400, 0)).toBe(0);
        expect(getCenteredScrollTopForIndex(layout, 200, 2)).toBe(182);
        expect(getCenteredScrollTopForIndex(layout, 400, 3)).toBe(48);
    });
});
