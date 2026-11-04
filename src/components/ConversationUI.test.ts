import { describe, expect, it, vi } from "vitest";
import {
    applyScrollHighlight,
    MESSAGE_SCROLL_START_GAP_PX,
    scrollMessageIntoView,
} from "@/components/conversation/scrollHighlight";
import { VIRTUAL_ROW_GAP_PX } from "@/components/conversation/virtualizedMessageListLayout";

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

    it("should pin the target below the viewport top with a gap smaller than the row gap when align is start", () => {
        const container = {
            scrollTop: 1000,
            getBoundingClientRect: () => ({ top: 60 }),
            scrollTo: vi.fn(),
        } as unknown as HTMLElement;
        const target = {
            getBoundingClientRect: () => ({ top: 460 }),
            scrollIntoView: vi.fn(),
        } as unknown as HTMLElement;

        scrollMessageIntoView(target, "start", container);

        expect(MESSAGE_SCROLL_START_GAP_PX).toBeGreaterThan(0);
        expect(MESSAGE_SCROLL_START_GAP_PX).toBeLessThan(VIRTUAL_ROW_GAP_PX);
        expect(container.scrollTo).toHaveBeenCalledWith({
            top: 1000 + 400 - MESSAGE_SCROLL_START_GAP_PX,
            behavior: "smooth",
        });
        expect(target.scrollIntoView).not.toHaveBeenCalled();
    });

    it("should clamp to zero when start-aligning the first message", () => {
        const container = {
            scrollTop: 0,
            getBoundingClientRect: () => ({ top: 0 }),
            scrollTo: vi.fn(),
        } as unknown as HTMLElement;
        const target = { getBoundingClientRect: () => ({ top: 2 }) } as unknown as HTMLElement;

        scrollMessageIntoView(target, "start", container);

        expect(container.scrollTo).toHaveBeenCalledWith({ top: 0, behavior: "smooth" });
    });
});
