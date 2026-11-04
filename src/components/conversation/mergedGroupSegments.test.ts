import { describe, expect, it } from "vitest";

import {
    MERGED_GROUP_SEGMENT_TARGET_PX,
    MERGED_GROUP_SPLIT_MIN_HEIGHT_PX,
    splitMergedGroupMessages,
} from "./mergedGroupSegments";
import {
    estimateMergedGroupHeight,
    estimateMergedSegmentHeight,
} from "./messageHeightEstimate";

function messagesWithHeights(heights: number[]) {
    const messages = heights.map((_, index) => ({ id: index + 1 }));
    const heightById = new Map(messages.map((message, index) => [message.id, heights[index]]));
    return { messages, heightById };
}

describe("splitMergedGroupMessages", () => {
    it("should keep a single row when the group is shorter than the split threshold", () => {
        const { messages, heightById } = messagesWithHeights([200, 200]);

        expect(
            splitMergedGroupMessages(messages, heightById, MERGED_GROUP_SPLIT_MIN_HEIGHT_PX - 1),
        ).toBeNull();
    });

    it("should split tall groups into contiguous runs near the target height", () => {
        const { messages, heightById } = messagesWithHeights([54, 82, 54, 102, 54, 54, 102, 54, 971]);

        const segments = splitMergedGroupMessages(messages, heightById, 1600)!;

        expect(segments.flat().map((message) => message.id)).toEqual(messages.map((m) => m.id));
        expect(segments.length).toBeGreaterThan(1);
        segments.slice(0, -1).forEach((segment) => {
            const height = segment.reduce((sum, message) => sum + heightById.get(message.id)!, 0);
            expect(height).toBeGreaterThanOrEqual(MERGED_GROUP_SEGMENT_TARGET_PX);
        });
    });

    it("should attach trailing zero-height parts to the previous segment when only hidden parts remain", () => {
        const { messages, heightById } = messagesWithHeights([400, 400, 0]);

        const segments = splitMergedGroupMessages(messages, heightById, 900)!;

        expect(segments.map((segment) => segment.map((message) => message.id))).toEqual([[1], [2, 3]]);
    });

    it("should return null when splitting would leave a single segment", () => {
        const { messages, heightById } = messagesWithHeights([1000, 0]);

        expect(splitMergedGroupMessages(messages, heightById, 1100)).toBeNull();
    });
});

describe("estimateMergedSegmentHeight", () => {
    it("should add up to the whole merged group estimate", () => {
        const runs = [[54, 82, 0, 300], [102, 54], [971]];

        const total = runs.reduce(
            (sum, parts, index) =>
                sum + estimateMergedSegmentHeight(parts, {
                    isFirst: index === 0,
                    isLast: index === runs.length - 1,
                }),
            0,
        );

        expect(total).toBe(estimateMergedGroupHeight(runs.flat()));
    });
});
