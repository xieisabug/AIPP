import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { followScrollContainerBottom } from "./pinScrollToBottom";

let resizeCallbacks: Array<() => void> = [];

class ResizeObserverMock {
    private readonly callback: () => void;

    constructor(callback: () => void) {
        this.callback = callback;
        resizeCallbacks.push(callback);
    }

    observe = vi.fn();
    disconnect = vi.fn(() => {
        resizeCallbacks = resizeCallbacks.filter((cb) => cb !== this.callback);
    });
}

function triggerResize() {
    resizeCallbacks.forEach((callback) => callback());
}

function makeContainer(initialScrollHeight: number, clientHeight = 400) {
    const container = document.createElement("div");
    let scrollHeight = initialScrollHeight;
    Object.defineProperty(container, "scrollHeight", {
        configurable: true,
        get: () => scrollHeight,
    });
    Object.defineProperty(container, "clientHeight", {
        configurable: true,
        value: clientHeight,
    });
    container.scrollTop = Math.max(0, scrollHeight - clientHeight);
    return {
        container,
        setScrollHeight: (value: number) => {
            scrollHeight = value;
        },
    };
}

describe("followScrollContainerBottom", () => {
    beforeEach(() => {
        resizeCallbacks = [];
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            cb(performance.now());
            return 1;
        });
        vi.stubGlobal("cancelAnimationFrame", vi.fn());
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    it("should re-pin to bottom when late content grows after initial pin", () => {
        const { container, setScrollHeight } = makeContainer(1000);
        const onScrollStateChange = vi.fn();

        const stop = followScrollContainerBottom({
            container,
            onScrollStateChange,
            shouldContinue: () => true,
        });

        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(1100);
        expect(onScrollStateChange).toHaveBeenCalledWith(container);
        stop();
    });

    it("should stop following when user shows scroll intent", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new Event("wheel"));
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(600);
    });

    it("should re-pin when a programmatic scroll over-compensates away from bottom", () => {
        const { container, setScrollHeight } = makeContainer(3000);

        const stop = followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        setScrollHeight(2500);
        container.scrollTop = 1200;
        container.dispatchEvent(new Event("scroll"));

        expect(container.scrollTop).toBe(2100);
        stop();
    });

    it("should not pull back after user scrolls away", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new Event("pointerdown"));
        container.scrollTop = 200;
        container.dispatchEvent(new Event("scroll"));
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(200);
    });

    it("should not pin when shouldContinue turns false", () => {
        const { container, setScrollHeight } = makeContainer(1000);
        let active = true;

        followScrollContainerBottom({
            container,
            shouldContinue: () => active,
        });

        active = false;
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(600);
    });

    it("should re-pin when scrollHeight grows without ResizeObserver", () => {
        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(1000);

        const stop = followScrollContainerBottom({
            container,
            onScrollStateChange: vi.fn(),
            shouldContinue: () => true,
        });

        setScrollHeight(1800);
        // 不触发 ResizeObserver，仅靠 height poll；rAF 仍走 stub 同步执行
        vi.advanceTimersByTime(100);

        expect(container.scrollTop).toBe(1400);
        stop();
        vi.useRealTimers();
    });

    it("should keep following across many stream-like height growths", () => {
        // 用异步 rAF，避免同步 stub 在 cb 把 frameRef 置空后又被赋回 id 导致后续 schedulePin 早退
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        vi.stubGlobal("cancelAnimationFrame", (id: number) => {
            void id;
            rafQueue.length = 0;
        });
        const flushRaf = () => {
            const queued = rafQueue.splice(0);
            queued.forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(1000);
        let streaming = true;

        const stop = followScrollContainerBottom({
            container,
            shouldContinue: () => streaming,
            maxDurationMs: 60_000,
        });

        for (const height of [1200, 1600, 2200, 3000, 4000]) {
            setScrollHeight(height);
            // 高度 poll（虚拟列表 spacer 不一定触发 ResizeObserver）
            vi.advanceTimersByTime(100);
            flushRaf();
            expect(container.scrollTop).toBe(height - 400);
        }

        streaming = false;
        setScrollHeight(4500);
        vi.advanceTimersByTime(100);
        flushRaf();
        // shouldContinue false → stop，不再贴底
        expect(container.scrollTop).toBe(3600);
        stop();
        vi.useRealTimers();
    });

});
