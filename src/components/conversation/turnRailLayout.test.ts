import { describe, expect, it } from "vitest";

import { sampleTickIndices, tickOffset, turnIndexFromOffset } from "./turnRailLayout";

describe("turnIndexFromOffset", () => {
    it("should snap to the nearest turn when offset is inside the track", () => {
        expect(turnIndexFromOffset(0, 100, 3)).toBe(0);
        expect(turnIndexFromOffset(40, 100, 3)).toBe(1);
        expect(turnIndexFromOffset(90, 100, 3)).toBe(2);
    });

    it("should clamp when offset is outside the track", () => {
        expect(turnIndexFromOffset(-30, 100, 3)).toBe(0);
        expect(turnIndexFromOffset(500, 100, 3)).toBe(2);
        expect(turnIndexFromOffset(50, 0, 3)).toBe(0);
    });
});

describe("tickOffset", () => {
    it("should spread turns evenly from top to bottom", () => {
        expect(tickOffset(0, 100, 5)).toBe(0);
        expect(tickOffset(2, 100, 5)).toBe(50);
        expect(tickOffset(4, 100, 5)).toBe(100);
    });

    it("should center a single turn", () => {
        expect(tickOffset(0, 0, 1)).toBe(0);
        expect(tickOffset(0, 40, 1)).toBe(20);
    });
});

describe("sampleTickIndices", () => {
    it("should draw every tick when spacing is wide enough", () => {
        // 4 轮铺在 48px：间距 16px ≥ 8px
        expect(sampleTickIndices(4, 48, 8)).toEqual([0, 1, 2, 3]);
    });

    it("should sample with a fixed stride and keep both ends when ticks are crowded", () => {
        // 201 轮铺在 400px：间距 2px，步长 4 → 0,4,…,200
        const indices = sampleTickIndices(201, 400, 8);
        expect(indices[0]).toBe(0);
        expect(indices[indices.length - 1]).toBe(200);
        expect(indices).toHaveLength(51);
        for (let i = 1; i < indices.length; i += 1) {
            expect((indices[i] - indices[i - 1]) * 2).toBeGreaterThanOrEqual(8);
        }
    });

    it("should replace the last sample instead of crowding it against the final turn", () => {
        // 10 轮铺在 45px：间距 5px，步长 2 → 0,2,4,6,8；8 与 9 只差 5px，改为 9
        expect(sampleTickIndices(10, 45, 8)).toEqual([0, 2, 4, 6, 9]);
    });

    it("should keep the tick count bounded by the track height", () => {
        const indices = sampleTickIndices(5000, 400, 8);
        expect(indices.length).toBeLessThanOrEqual(400 / 8 + 1);
        expect(indices[indices.length - 1]).toBe(4999);
    });

    it("should handle empty, single and unmeasured tracks", () => {
        expect(sampleTickIndices(0, 100, 8)).toEqual([]);
        expect(sampleTickIndices(1, 0, 8)).toEqual([0]);
        expect(sampleTickIndices(5, 0, 8)).toEqual([0, 4]);
    });
});
