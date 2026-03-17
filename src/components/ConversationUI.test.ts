import { describe, expect, it, vi } from "vitest";
import { applyScrollHighlight } from "@/components/conversation/scrollHighlight";

describe("ConversationUI scroll highlight helper", () => {
    it("scrolls to the target, applies a temporary highlight, and clears it after 2 seconds", () => {
        vi.useFakeTimers();

        const target = {
            scrollIntoView: vi.fn(),
        } as unknown as HTMLElement;
        const highlightUpdates: Set<number>[] = [];
        const setShiningMessageIds = vi.fn((value: Set<number> | ((prev: Set<number>) => Set<number>)) => {
            const resolved = typeof value === "function" ? value(new Set<number>()) : value;
            highlightUpdates.push(new Set(resolved));
        });
        const clearPendingScrollMessageId = vi.fn();

        applyScrollHighlight({
            target,
            messageId: 42,
            setShiningMessageIds,
            clearPendingScrollMessageId,
            requestFrame: (callback) => {
                callback(0);
                return 1;
            },
        });

        expect(target.scrollIntoView).toHaveBeenCalledWith({ behavior: "smooth", block: "center" });
        expect(highlightUpdates).toHaveLength(1);
        expect(highlightUpdates[0]).toEqual(new Set([42]));
        expect(clearPendingScrollMessageId).toHaveBeenCalledWith(null);

        vi.advanceTimersByTime(1999);
        expect(highlightUpdates).toHaveLength(1);

        vi.advanceTimersByTime(1);
        expect(highlightUpdates).toHaveLength(2);
        expect(highlightUpdates[1]).toEqual(new Set());

        vi.useRealTimers();
    });
});
