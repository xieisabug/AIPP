import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
    followScrollContainerBottom,
    pinScrollContainerToBottom,
} from "./pinScrollToBottom";

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

function placeContainer(
    container: HTMLDivElement,
    width: number,
    height: number,
    scrollbar = 0,
) {
    Object.defineProperty(container, "offsetWidth", {
        configurable: true,
        value: width,
    });
    Object.defineProperty(container, "clientWidth", {
        configurable: true,
        value: width - scrollbar,
    });
    container.getBoundingClientRect = () => ({
        x: 0,
        y: 0,
        left: 0,
        top: 0,
        right: width,
        bottom: height,
        width,
        height,
        toJSON() {
            return {};
        },
    }) as DOMRect;
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
        vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response())));
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    it.each([false, true])("preserves follow intent across a zero-size hidden preview (paused=%s)", (paused) => {
        vi.useFakeTimers();
        const shell = document.createElement("div");
        shell.setAttribute("data-aipp-mobile-shell", "");
        const page = document.createElement("div");
        const container = document.createElement("div");
        shell.append(page); page.append(container); document.body.append(shell);
        let height = 2000;
        let top = 1600;
        Object.defineProperties(container, {
            scrollHeight: { get: () => page.hidden ? 0 : height },
            clientHeight: { get: () => page.hidden ? 0 : 400 },
            scrollTop: { get: () => page.hidden ? 0 : top, set: value => { top = value; } },
        });
        const onUserResume = vi.fn();
        const stop = followScrollContainerBottom({ container, shouldContinue: () => true, onUserResume });
        try {
            if (paused) {
                container.dispatchEvent(new Event("touchstart"));
                container.scrollTop = 900;
                container.dispatchEvent(new Event("scroll"));
                window.dispatchEvent(new Event("touchend"));
            }
            page.hidden = true;
            container.dispatchEvent(new Event("scroll"));
            height = 2400;
            triggerResize();
            vi.advanceTimersByTime(1000);
            expect(onUserResume).not.toHaveBeenCalled();
            page.hidden = false;
            container.dispatchEvent(new Event("scroll"));
            triggerResize();
            vi.advanceTimersByTime(200);
            expect(container.scrollTop).toBe(paused ? 900 : 2000);
            if (paused) {
                container.dispatchEvent(new Event("touchstart"));
                container.scrollTop = 2000;
                container.dispatchEvent(new Event("scroll"));
                window.dispatchEvent(new Event("touchend"));
                height = 2600;
                triggerResize();
                vi.advanceTimersByTime(200);
                expect(container.scrollTop).toBe(2200);
                expect(onUserResume).toHaveBeenCalledTimes(1);
            }
        } finally {
            stop(); shell.remove(); vi.useRealTimers();
        }
    });

    it("does not follow stream growth while the mobile page is hidden", () => {
        const { container, setScrollHeight } = makeContainer(1000);
        const shell = document.createElement("div");
        shell.setAttribute("data-aipp-mobile-shell", "");
        const page = document.createElement("div");
        shell.append(page);
        page.append(container);
        document.body.append(shell);
        const stop = followScrollContainerBottom({ container, shouldContinue: () => true });
        page.hidden = true;
        container.scrollTop = 200;
        setScrollHeight(1600);
        triggerResize();
        expect(container.scrollTop).toBe(200);
        stop();
        shell.remove();
    });

    it("does not pin after the mobile page becomes hidden", () => {
        const frames: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback) => { frames.push(callback); return frames.length; });
        const { container, setScrollHeight } = makeContainer(1000);
        const shell = document.createElement("div");
        shell.setAttribute("data-aipp-mobile-shell", "");
        const page = document.createElement("div");
        shell.append(page);
        page.append(container);
        document.body.append(shell);
        const stop = pinScrollContainerToBottom({ container, shouldContinue: () => true });
        page.hidden = true;
        container.scrollTop = 200;
        setScrollHeight(1600);
        triggerResize();
        frames.splice(0).forEach(callback => callback(0));
        expect(container.scrollTop).toBe(200);
        stop();
        shell.remove();
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

    it("should pause following when user wheels upward", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(
            new WheelEvent("wheel", { deltaY: -80 }),
        );
        setScrollHeight(1500);
        triggerResize();

        // paused：增高不再抢滚
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

    it("should not pull back after user scrolls away via pointer", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        // 先离底再 pointerdown：模拟拖动滚动条/触摸上滑后的意图
        container.scrollTop = 200;
        container.dispatchEvent(new Event("pointerdown"));
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

    it("should resume following after user scrolls back to bottom mid-stream", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            const queued = rafQueue.splice(0);
            queued.forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        // 上滑暂停
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
        container.scrollTop = 200;
        container.dispatchEvent(new Event("scroll"));

        // 暂停期间内容持续增高（poll 不得吃掉 delta）
        setScrollHeight(1800);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(200);

        // 用户向下滚回底部（wheel-down force resume，越过上滑锁）
        container.scrollTop = 1800 - 400;
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));
        flushRaf();
        expect(container.scrollTop).toBe(1400);

        // 流式继续增高 → 必须持续贴底（case 3：resume 后 viewport 跟着 MARK 走）
        for (const height of [2200, 2800, 3600, 4500]) {
            setScrollHeight(height);
            vi.advanceTimersByTime(100);
            flushRaf();
            expect(container.scrollTop).toBe(height - 400);
        }
        vi.useRealTimers();
    });

    it("should resume on wheel-down near bottom then keep following growth", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            rafQueue.splice(0).forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(2000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 }));
        container.scrollTop = 300;
        setScrollHeight(3000);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(300);

        // 向下滚到距底 50px（宽松 resume 阈值内）
        container.scrollTop = 3000 - 400 - 50;
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
        flushRaf(); // wheel-down rAF resume

        setScrollHeight(4000);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(3600);

        setScrollHeight(5000);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(4600);
        vi.useRealTimers();
    });

    it("should ignore scroll-near-bottom resume briefly after wheel-up, but honor near-bottom wheel-down", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            rafQueue.splice(0).forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({
            toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout", "performance"],
        });
        const { container, setScrollHeight } = makeContainer(2000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 }));
        container.scrollTop = 300;
        // 立刻滚到近底：锁定期内不得 resume
        container.scrollTop = 2000 - 400 - 20;
        container.dispatchEvent(new Event("scroll"));
        flushRaf();
        setScrollHeight(3000);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(1580); // 仍暂停在近底位置，未贴 2600

        // 内容已长高，当前位置已不在近底：中段下滚不得拽到底
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
        flushRaf();
        expect(container.scrollTop).toBe(1580);

        // 真正滚到新底部附近后再下滚 → 恢复跟滚（force 越过上滑锁）
        container.scrollTop = 3000 - 400 - 20;
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
        flushRaf();
        setScrollHeight(4000);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(3600);
        vi.useRealTimers();
    });

    it("should pause on touchstart when already away from bottom", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.scrollTop = 150;
        container.dispatchEvent(new Event("touchstart"));
        setScrollHeight(1600);
        triggerResize();

        expect(container.scrollTop).toBe(150);
    });

    it("should not pause on wheel-down while still at bottom", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        // 在底部向下滚（已到底）不应暂停
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(1100);
    });

    it("should keep following under fast burst height growth", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            const queued = rafQueue.splice(0);
            queued.forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(800);

        const stop = followScrollContainerBottom({
            container,
            shouldContinue: () => true,
            maxDurationMs: 60_000,
        });

        // 模拟低延迟突发 token：短间隔多次跳高（靠 height poll，16ms 帧内也可能叠多跳）
        let height = 800;
        for (let i = 0; i < 20; i += 1) {
            height += 50 + (i % 3) * 30;
            setScrollHeight(height);
            vi.advanceTimersByTime(100); // height poll 周期
            flushRaf();
            expect(container.scrollTop).toBe(height - 400);
        }

        stop();
        vi.useRealTimers();
    });

    it("should resume via height poll when user is at bottom without a scroll event", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            const queued = rafQueue.splice(0);
            queued.forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({
            toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout", "performance"],
        });
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -80 }));
        container.scrollTop = 100;
        setScrollHeight(1600);
        vi.advanceTimersByTime(100);
        flushRaf();
        // 仍暂停
        expect(container.scrollTop).toBe(100);

        // End 键贴底但不派发 scroll；越过上滑锁后 poll 恢复
        container.scrollTop = 1600 - 400;
        vi.advanceTimersByTime(500);
        flushRaf();
        expect(container.scrollTop).toBe(1200);

        setScrollHeight(2200);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(1800);
        vi.useRealTimers();
    });

    it("should not pause on End key so jump-to-bottom can keep following", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            rafQueue.splice(0).forEach((cb) => cb(performance.now()));
        };

        const { container, setScrollHeight } = makeContainer(1000);
        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.scrollTop = 200;
        container.dispatchEvent(new KeyboardEvent("keydown", { key: "End" }));
        // 模拟 End 贴底
        container.scrollTop = 600;
        container.dispatchEvent(new Event("scroll"));

        setScrollHeight(1500);
        triggerResize();
        flushRaf();
        expect(container.scrollTop).toBe(1100);
    });

    it("should not jump to bottom on post-send up-then-tiny-down within one viewport", () => {
        // 复现：发消息贴底跟滚 → 上滑一段（仍在一屏内）→ 轻微下滚 → 旧 oneScreen 逻辑会误贴底
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            rafQueue.splice(0).forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(3000, 800);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        // 上滑约 500px：距底仍 < clientHeight，旧逻辑会把后续任何下滚当成 resume
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
        container.scrollTop = 3000 - 800 - 500;
        container.dispatchEvent(new Event("scroll"));
        expect(container.scrollTop).toBe(1700);

        // 轻微下滚 40px，距底仍约 460（> resume 240）
        container.scrollTop = 1740;
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 40 }));
        container.dispatchEvent(new Event("scroll"));
        flushRaf();
        expect(container.scrollTop).toBe(1740);

        setScrollHeight(3600);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(1740);
        vi.useRealTimers();
    });

    it("should not jump to bottom on mid-history wheel-down", () => {
        let rafId = 0;
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            rafId += 1;
            return rafId;
        });
        const flushRaf = () => {
            rafQueue.splice(0).forEach((cb) => cb(performance.now()));
        };

        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        const { container, setScrollHeight } = makeContainer(3000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -120 }));
        container.scrollTop = 200;
        container.dispatchEvent(new Event("scroll"));

        // 中段下滚：不得贴底
        container.scrollTop = 280;
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: 80 }));
        flushRaf();
        expect(container.scrollTop).toBe(280);

        setScrollHeight(4000);
        vi.advanceTimersByTime(100);
        flushRaf();
        expect(container.scrollTop).toBe(280);
        vi.useRealTimers();
    });

    it("should pause when the scrollbar moves up from the bottom without a height change", () => {
        const { container, setScrollHeight } = makeContainer(1000);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        // 仍在 240px 恢复阈值内，但高度没变，是滚动条在往上拖
        container.scrollTop = 480;
        container.dispatchEvent(new Event("scroll"));
        container.scrollTop = 300;
        container.dispatchEvent(new Event("scroll"));
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(300);
    });

    it("should pause on a scrollbar pointerdown while still at the bottom", () => {
        const { container, setScrollHeight } = makeContainer(1000);
        placeContainer(container, 800, 400, 16);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new PointerEvent("pointerdown", {
            clientX: 796,
            clientY: 120,
            bubbles: true,
        }));
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(600);
    });

    it("should keep following when a content pointerdown happens at the bottom", () => {
        const { container, setScrollHeight } = makeContainer(1000);
        placeContainer(container, 800, 400, 16);

        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
        });

        container.dispatchEvent(new PointerEvent("pointerdown", {
            clientX: 40,
            clientY: 20,
            bubbles: true,
        }));
        setScrollHeight(1500);
        triggerResize();

        expect(container.scrollTop).toBe(1100);
    });
});

describe("pinScrollContainerToBottom", () => {
    beforeEach(() => {
        resizeCallbacks = [];
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
        vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response())));
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    function queueAnimationFrames() {
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            return rafQueue.length;
        });
        vi.stubGlobal("cancelAnimationFrame", vi.fn());
        return () => {
            const queued = rafQueue.splice(0);
            queued.forEach((cb) => cb(performance.now()));
        };
    }

    it("should release the pin when the user scrolls up without a height change", () => {
        const flushRaf = queueAnimationFrames();
        const { container } = makeContainer(1000);
        const onComplete = vi.fn();
        const onUserRelease = vi.fn();

        pinScrollContainerToBottom({
            container,
            shouldContinue: () => true,
            onComplete,
            onUserRelease,
            minFrameCount: 10,
            stableFrameCount: 4,
            maxFrameCount: 30,
        });

        container.scrollTop = 120;
        container.dispatchEvent(new Event("scroll"));
        flushRaf();

        expect(onUserRelease).toHaveBeenCalledTimes(1);
        expect(onComplete).not.toHaveBeenCalled();
        expect(container.scrollTop).toBe(120);
    });

    it("should release the pin when the user wheels upward", () => {
        const flushRaf = queueAnimationFrames();
        const { container } = makeContainer(1000);
        const onComplete = vi.fn();
        const onUserRelease = vi.fn();

        pinScrollContainerToBottom({
            container,
            shouldContinue: () => true,
            onComplete,
            onUserRelease,
        });

        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -40 }));
        container.scrollTop = 80;
        flushRaf();

        expect(onUserRelease).toHaveBeenCalledTimes(1);
        expect(onComplete).not.toHaveBeenCalled();
        expect(container.scrollTop).toBe(80);
    });

    it("reports each programmatic jump synchronously so callers can compensate visually", () => {
        const flushRaf = queueAnimationFrames();
        const { container, setScrollHeight } = makeContainer(1000);
        container.scrollTop = 100;
        const deltas: number[] = [];

        pinScrollContainerToBottom({
            container,
            shouldContinue: () => true,
            onScrollDelta: (delta) => deltas.push(delta),
        });

        // 首次调用同步跳到最终位置（落点可靠），并在同一任务里报告位移
        expect(container.scrollTop).toBe(600);
        expect(deltas).toEqual([500]);

        setScrollHeight(1200);
        flushRaf();
        expect(container.scrollTop).toBe(800);
        expect(deltas).toEqual([500, 200]);

        // 没有移动时不报告
        flushRaf();
        expect(deltas).toEqual([500, 200]);
    });

    it("keeps pinning without counting stable frames while holdWhile is true", () => {
        const flushRaf = queueAnimationFrames();
        const { container } = makeContainer(1000);
        const onComplete = vi.fn();
        let holding = true;

        pinScrollContainerToBottom({
            container,
            shouldContinue: () => true,
            onComplete,
            holdWhile: () => holding,
            minFrameCount: 2,
            stableFrameCount: 1,
            maxFrameCount: 3,
        });

        for (let i = 0; i < 20; i += 1) {
            flushRaf();
        }
        expect(onComplete).not.toHaveBeenCalled();

        holding = false;
        for (let i = 0; i < 5; i += 1) {
            flushRaf();
        }
        expect(onComplete).toHaveBeenCalledTimes(1);
    });

    it("should release the pin on touchmove", () => {
        const flushRaf = queueAnimationFrames();
        const { container, setScrollHeight } = makeContainer(1000);
        const onUserRelease = vi.fn();

        pinScrollContainerToBottom({
            container,
            shouldContinue: () => true,
            onUserRelease,
        });

        container.dispatchEvent(new Event("touchmove"));
        container.scrollTop = 300;
        setScrollHeight(1200);
        triggerResize();
        flushRaf();

        expect(onUserRelease).toHaveBeenCalledTimes(1);
        expect(container.scrollTop).toBe(300);
    });
});

describe("followScrollContainerBottom resume / suspend", () => {
    beforeEach(() => {
        resizeCallbacks = [];
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    });

    afterEach(() => {
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
        vi.useRealTimers();
    });

    function setup(initialScrollHeight = 2000) {
        const rafQueue: FrameRequestCallback[] = [];
        vi.stubGlobal("requestAnimationFrame", (cb: FrameRequestCallback) => {
            rafQueue.push(cb);
            return rafQueue.length;
        });
        vi.stubGlobal("cancelAnimationFrame", vi.fn());
        vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "setTimeout", "clearTimeout"] });
        let now = 0;
        vi.spyOn(performance, "now").mockImplementation(() => now);
        const flushRaf = () => {
            rafQueue.splice(0).forEach((cb) => cb(now));
        };
        const advance = (ms: number) => {
            now += ms;
            vi.advanceTimersByTime(ms);
            flushRaf();
        };
        return { ...makeContainer(initialScrollHeight), flushRaf, advance };
    }

    it("should not resume from a small downward scroll that stays off the bottom", () => {
        const { container, setScrollHeight, advance } = setup();
        followScrollContainerBottom({ container, shouldContinue: () => true });

        // 用户上滑 150px
        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 }));
        container.scrollTop = 1450;
        container.dispatchEvent(new Event("scroll"));
        advance(600); // 越过上滑锁

        // Virtuoso 校正 / 惯性让 scrollTop 下移 50px：距底 100，仍在旧 240 阈值内
        container.scrollTop = 1500;
        container.dispatchEvent(new Event("scroll"));
        setScrollHeight(2300);
        advance(100);

        expect(container.scrollTop).toBe(1500);
    });

    it("should resume once the user actually scrolls back to the bottom", () => {
        const { container, setScrollHeight, advance } = setup();
        followScrollContainerBottom({ container, shouldContinue: () => true });

        container.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 }));
        container.scrollTop = 1400;
        container.dispatchEvent(new Event("scroll"));
        advance(600);

        container.scrollTop = 1590; // 距底 10px，向下
        container.dispatchEvent(new Event("scroll"));
        setScrollHeight(2500);
        advance(100);

        expect(container.scrollTop).toBe(2100);
    });

    it("should pause when a touch drags upward while content keeps growing", () => {
        const { container, setScrollHeight, advance } = setup();
        followScrollContainerBottom({ container, shouldContinue: () => true });

        container.dispatchEvent(new Event("touchstart"));
        setScrollHeight(2100);
        container.scrollTop = 1500;
        container.dispatchEvent(new Event("scroll"));
        advance(100);
        expect(container.scrollTop).toBe(1500);

        // 松手后仍不在底部：保持暂停
        window.dispatchEvent(new Event("touchend"));
        setScrollHeight(2400);
        advance(100);
        expect(container.scrollTop).toBe(1500);
    });

    it("should hold pinning while suspended, then catch up", () => {
        const { container, setScrollHeight, advance } = setup();
        let suspended = true;
        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
            suspendWhile: () => suspended,
        });

        container.scrollTop = 1200; // 动画中途位置
        setScrollHeight(2200);
        advance(100);
        expect(container.scrollTop).toBe(1200);

        suspended = false;
        advance(16);
        expect(container.scrollTop).toBe(1800);
    });

    it("should not pull the viewport down when started paused", () => {
        const { container, setScrollHeight, advance } = setup();
        container.scrollTop = 900;
        followScrollContainerBottom({
            container,
            shouldContinue: () => true,
            startPaused: true,
        });

        setScrollHeight(2600);
        advance(200);
        expect(container.scrollTop).toBe(900);
    });
});
