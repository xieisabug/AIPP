import { describe, expect, it } from "vitest";

import { railOffsetTop, tickOffset, tickThickness, turnIndexFromOffset } from "./turnRailLayout";

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

describe("railOffsetTop", () => {
    it("should center the track plus its padding inside the message area", () => {
        // 32px 轨道 + 上下各 6px，在 800px 消息区里
        expect(railOffsetTop(32, 800, 6)).toBe(378);
        expect(railOffsetTop(400, 800, 6)).toBe(194);
    });

    it("should clamp to the top when the message area is shorter than the track", () => {
        expect(railOffsetTop(400, 100, 6)).toBe(0);
        expect(railOffsetTop(0, 0, 6)).toBe(0);
    });
});

describe("tickThickness", () => {
    it("should use the full 3px tick while turns have room", () => {
        // 4 轮铺在 48px：间距 16px
        expect(tickThickness(4, 48)).toBe(3);
        expect(tickThickness(1, 0)).toBe(3);
        expect(tickThickness(2, 0)).toBe(3);
    });

    it("should thin ticks instead of dropping turns when they crowd together", () => {
        // 101 轮铺在 400px：间距 4px，刻度 2px + 空隙 2px
        expect(tickThickness(101, 400)).toBe(2);
        // 301 轮铺在 400px：间距 1.33px，最细 1px
        expect(tickThickness(301, 400)).toBe(1);
    });
});
