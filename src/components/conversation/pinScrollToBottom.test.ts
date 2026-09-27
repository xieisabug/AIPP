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
});
