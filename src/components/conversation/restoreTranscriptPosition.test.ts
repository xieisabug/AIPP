import { afterEach, expect, it, vi } from "vitest";
import { restoreTranscriptPosition } from "./restoreTranscriptPosition";

afterEach(() => vi.useRealTimers());

it("should preserve the percentage through measurement and yield immediately to the reader", () => {
    vi.useFakeTimers();
    const container = document.createElement("div");
    Object.defineProperty(container, "clientHeight", { value: 200 });
    let height = 1000;
    Object.defineProperty(container, "scrollHeight", { get: () => height });
    restoreTranscriptPosition(container, 0.75, () => {});
    expect(container.scrollTop).toBe(600);
    height = 1800;
    vi.advanceTimersToNextFrame();
    expect(container.scrollTop).toBe(1200);
    container.dispatchEvent(new Event("wheel"));
    container.scrollTop = 900;
    vi.advanceTimersByTime(600);
    expect(container.scrollTop).toBe(900);
});
