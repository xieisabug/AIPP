export interface PinScrollToBottomOptions {
    container: HTMLDivElement;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    shouldContinue: () => boolean;
    onComplete?: () => void;
    minFrameCount?: number;
    stableFrameCount?: number;
    maxFrameCount?: number;
    observeMode?: "full" | "tail";
    /** 硬超时：pin 被打断时也不能永久卡在未完成状态（如 visibility:hidden） */
    failsafeTimeoutMs?: number;
}

const DEFAULT_MIN_FRAME_COUNT = 10;
const DEFAULT_STABLE_FRAME_COUNT = 4;
const DEFAULT_MAX_FRAME_COUNT = 30;

function observeTailElements(
    container: HTMLDivElement,
    observedElements: Set<Element>,
    resizeObserver: ResizeObserver,
) {
    const lastReplyContainer = container.querySelector(
        "[data-aipp-slot='chat-last-reply-container']",
    );
    if (lastReplyContainer && !observedElements.has(lastReplyContainer)) {
        observedElements.add(lastReplyContainer);
        resizeObserver.observe(lastReplyContainer);
    }

    const footer = lastReplyContainer?.parentElement;
    if (footer && !observedElements.has(footer)) {
        observedElements.add(footer);
        resizeObserver.observe(footer);
    }

    if (!observedElements.has(container)) {
        observedElements.add(container);
        resizeObserver.observe(container);
    }
}

function observeAllContentElements(
    container: HTMLDivElement,
    observedElements: Set<Element>,
    resizeObserver: ResizeObserver,
) {
    const elements = [
        ...Array.from(container.children),
        ...Array.from(container.querySelectorAll("*")),
    ];
    elements.forEach((element) => {
        if (observedElements.has(element)) {
            return;
        }

        observedElements.add(element);
        resizeObserver.observe(element);
    });
}

export function pinScrollContainerToBottom({
    container,
    onScrollStateChange,
    shouldContinue,
    onComplete,
    minFrameCount = DEFAULT_MIN_FRAME_COUNT,
    stableFrameCount = DEFAULT_STABLE_FRAME_COUNT,
    maxFrameCount = DEFAULT_MAX_FRAME_COUNT,
    observeMode = "full",
    failsafeTimeoutMs,
}: PinScrollToBottomOptions): () => void {
    let elapsedFrames = 0;
    let stableFrames = 0;
    let lastMaxScrollTop: number | null = null;
    let isPinningScroll = false;
    let frameRef: number | null = null;
    let failsafeTimerId: number | null = null;
    let completed = false;

    const pinToBottom = () => {
        if (!shouldContinue()) {
            return;
        }

        isPinningScroll = true;
        container.scrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );
        isPinningScroll = false;
        onScrollStateChange?.(container);
    };

    const handlePinnedScroll = () => {
        if (isPinningScroll || !shouldContinue()) {
            return;
        }

        pinToBottom();
    };

    const observedElements = new Set<Element>();
    const contentResizeObserver = new ResizeObserver(() => {
        if (!shouldContinue()) {
            return;
        }

        pinToBottom();
    });

    const observeContentElements = () => {
        if (observeMode === "tail") {
            observeTailElements(container, observedElements, contentResizeObserver);
            return;
        }

        observeAllContentElements(container, observedElements, contentResizeObserver);
    };

    const mutationObserver = new MutationObserver(() => {
        if (!shouldContinue()) {
            return;
        }

        observeContentElements();
        pinToBottom();
    });

    const cleanup = () => {
        container.removeEventListener("scroll", handlePinnedScroll);
        contentResizeObserver.disconnect();
        mutationObserver.disconnect();
        if (frameRef !== null) {
            cancelAnimationFrame(frameRef);
            frameRef = null;
        }
        if (failsafeTimerId !== null) {
            window.clearTimeout(failsafeTimerId);
            failsafeTimerId = null;
        }
    };

    const finish = () => {
        if (completed) {
            return;
        }
        completed = true;
        cleanup();
        onComplete?.();
    };

    const scrollToBottom = () => {
        frameRef = null;
        if (!shouldContinue()) {
            return;
        }

        pinToBottom();
        elapsedFrames += 1;

        const maxScrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );
        // maxScrollTop === 0（短会话装得下）也应计为稳定，否则只能干等 max frame
        if (
            lastMaxScrollTop !== null
            && Math.abs(maxScrollTop - lastMaxScrollTop) <= 1
        ) {
            stableFrames += 1;
        } else {
            stableFrames = 0;
        }
        lastMaxScrollTop = maxScrollTop;

        const canStopAfterStableLayout =
            elapsedFrames >= minFrameCount
            && stableFrames >= stableFrameCount;
        const reachedMaxWait = elapsedFrames >= maxFrameCount;
        if (canStopAfterStableLayout || reachedMaxWait) {
            finish();
            return;
        }

        frameRef = requestAnimationFrame(scrollToBottom);
    };

    container.addEventListener("scroll", handlePinnedScroll, { passive: true });
    observeContentElements();
    mutationObserver.observe(container, {
        childList: true,
        subtree: true,
    });

    if (failsafeTimeoutMs != null && failsafeTimeoutMs > 0) {
        failsafeTimerId = window.setTimeout(() => {
            if (completed || !shouldContinue()) {
                return;
            }
            pinToBottom();
            finish();
        }, failsafeTimeoutMs);
    }

    scrollToBottom();

    return () => {
        if (completed) {
            return;
        }
        cleanup();
    };
}

export interface FollowScrollToBottomOptions {
    container: HTMLDivElement;
    /** 额外观察尺寸变化的内容元素（如虚拟列表外层），其长高不一定触发容器自身 resize */
    contentElements?: Array<Element | null | undefined>;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    shouldContinue: () => boolean;
    maxDurationMs?: number;
    bottomThresholdPx?: number;
}

const DEFAULT_FOLLOW_MAX_DURATION_MS = 10_000;
const DEFAULT_FOLLOW_BOTTOM_THRESHOLD_PX = 10;
const USER_INTENT_EVENTS = ["wheel", "touchstart", "pointerdown", "keydown"] as const;

/**
 * 初始贴底结束后，虚拟列表实测高度与估算不一致会持续收缩/增长，
 * 异步内容（代码高亮、图片、富文本预览）也可能继续长高。
 * 用户未主动交互前，内容尺寸变化或程序触发的滚动离开底部都会重新贴底；
 * 用户一旦滚轮/触摸/点击/按键，立即让位。
 */
export function followScrollContainerBottom({
    container,
    contentElements = [],
    onScrollStateChange,
    shouldContinue,
    maxDurationMs = DEFAULT_FOLLOW_MAX_DURATION_MS,
    bottomThresholdPx = DEFAULT_FOLLOW_BOTTOM_THRESHOLD_PX,
}: FollowScrollToBottomOptions): () => void {
    let stopped = false;
    let frameRef: number | null = null;

    const getDistanceToBottom = () =>
        container.scrollHeight - container.scrollTop - container.clientHeight;

    const pinToBottom = () => {
        frameRef = null;
        if (stopped || !shouldContinue()) {
            return;
        }

        const maxScrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );
        if (Math.abs(container.scrollTop - maxScrollTop) <= 1) {
            return;
        }

        container.scrollTop = maxScrollTop;
        onScrollStateChange?.(container);
    };

    const schedulePin = () => {
        if (stopped || frameRef !== null) {
            return;
        }
        frameRef = requestAnimationFrame(pinToBottom);
    };

    // 高度收缩时 Virtuoso / 浏览器滚动锚定会补偿 scrollTop，且可能过量补偿到远离底部；
    // 真正的用户滚动都会先经过 USER_INTENT_EVENTS 而停止跟随，这里的滚动都视为程序触发
    const handleScroll = () => {
        if (stopped || getDistanceToBottom() <= bottomThresholdPx) {
            return;
        }
        schedulePin();
    };

    const resizeObserver = new ResizeObserver(() => {
        if (!shouldContinue()) {
            stop();
            return;
        }
        schedulePin();
    });

    const observed = new Set<Element>();
    [container, ...Array.from(container.children), ...contentElements].forEach(
        (element) => {
            if (element && !observed.has(element)) {
                observed.add(element);
                resizeObserver.observe(element);
            }
        },
    );

    const timeoutId = window.setTimeout(() => stop(), maxDurationMs);

    function stop() {
        if (stopped) {
            return;
        }
        stopped = true;
        resizeObserver.disconnect();
        container.removeEventListener("scroll", handleScroll);
        USER_INTENT_EVENTS.forEach((eventName) => {
            container.removeEventListener(eventName, stop);
        });
        if (frameRef !== null) {
            cancelAnimationFrame(frameRef);
            frameRef = null;
        }
        window.clearTimeout(timeoutId);
    }

    container.addEventListener("scroll", handleScroll, { passive: true });
    USER_INTENT_EVENTS.forEach((eventName) => {
        container.addEventListener(eventName, stop, { passive: true });
    });

    return stop;
}
