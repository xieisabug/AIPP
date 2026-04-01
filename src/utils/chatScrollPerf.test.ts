import { describe, expect, it } from "vitest";

import { summarizeFrameDurations } from "./chatScrollPerf";

describe("chatScrollPerf", () => {
    it("summarizes frame durations with dropped frame estimates", () => {
        const summary = summarizeFrameDurations(
            [16, 17, 20, 35, 55],
            1000 / 60,
        );

        expect(summary.sampleCount).toBe(5);
        expect(summary.averageFrameMs).toBe(28.6);
        expect(summary.p95FrameMs).toBe(55);
        expect(summary.worstFrameMs).toBe(55);
        expect(summary.overBudgetFrameCount).toBe(4);
        expect(summary.severeFrameCount).toBe(2);
        expect(summary.estimatedDroppedFrameCount).toBe(3);
    });

    it("returns an empty summary when there are no frame samples", () => {
        expect(summarizeFrameDurations([])).toEqual({
            sampleCount: 0,
            averageFrameMs: 0,
            p95FrameMs: 0,
            worstFrameMs: 0,
            overBudgetFrameCount: 0,
            severeFrameCount: 0,
            estimatedDroppedFrameCount: 0,
        });
    });
});
