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
    /** 回底恢复跟滚的宽松阈值（Virtuoso 估高校正常留下数十 px 离底） */
    resumeBottomThresholdPx?: number;
}

const DEFAULT_FOLLOW_MAX_DURATION_MS = 10_000;
const DEFAULT_FOLLOW_BOTTOM_THRESHOLD_PX = 10;
/** 恢复跟滚用更宽阈值：贴底后 Virtuoso spacer/footer 常让 distance 卡在 10~60px */
const DEFAULT_RESUME_BOTTOM_THRESHOLD_PX = 240;
const USER_INTENT_EVENTS = ["wheel", "touchstart", "pointerdown", "keydown"] as const;

/**
 * 初始贴底结束后，虚拟列表实测高度与估算不一致会持续收缩/增长，
 * 异步内容（代码高亮、图片、富文本预览）也可能继续长高。
 * 用户滚轮向上 / 触摸离底后暂停跟滚；滚回底部（含宽松阈值）后自动恢复。
 */
export function followScrollContainerBottom({
    container,
    contentElements = [],
    onScrollStateChange,
    shouldContinue,
    maxDurationMs = DEFAULT_FOLLOW_MAX_DURATION_MS,
    bottomThresholdPx = DEFAULT_FOLLOW_BOTTOM_THRESHOLD_PX,
    resumeBottomThresholdPx = DEFAULT_RESUME_BOTTOM_THRESHOLD_PX,
}: FollowScrollToBottomOptions): () => void {
    let stopped = false;
    /** 用户上滑后暂停跟滚；贴底后自动恢复（与 useScrollManagement.isUserScrolledUp 对齐） */
    let pausedByUser = false;
    let isPinningScroll = false;
    /** rAF id；用 pinScheduled 判断是否已排队，避免同步 rAF stub 把 id 赋回后永久卡住 */
    let frameRef: number | null = null;
    let pinScheduled = false;
    let lastScrollHeight = container.scrollHeight;
    let scrollTopAtPause = container.scrollTop;
    /** wheel-up 后短暂禁止 scroll/poll 自动 resume，避免 Virtuoso 尚未离底时立刻解暂停 */
    let resumeLockedUntil = 0;

    const getDistanceToBottom = () =>
        container.scrollHeight - container.scrollTop - container.clientHeight;

    const isAtBottom = () => getDistanceToBottom() <= bottomThresholdPx;
    const isNearBottomForResume = () =>
        getDistanceToBottom() <= resumeBottomThresholdPx;

    const pinToBottom = () => {
        if (stopped || pausedByUser || !shouldContinue()) {
            return;
        }

        const maxScrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );
        if (Math.abs(container.scrollTop - maxScrollTop) <= 1) {
            return;
        }

        isPinningScroll = true;
        container.scrollTop = maxScrollTop;
        isPinningScroll = false;
        onScrollStateChange?.(container);
    };

    const schedulePin = () => {
        if (stopped || pausedByUser || pinScheduled) {
            return;
        }
        pinScheduled = true;
        frameRef = requestAnimationFrame(() => {
            frameRef = null;
            pinScheduled = false;
            pinToBottom();
        });
    };

    const resumeFollow = (reason: string, opts?: { force?: boolean }) => {
        if (stopped || !pausedByUser) {
            return;
        }
        if (!opts?.force && performance.now() < resumeLockedUntil) {
            return;
        }
        resumeLockedUntil = 0;
        pausedByUser = false;
        // 强制下一轮 poll 看到 delta（避免 resume 时 sync lastScrollHeight 吞掉已发生的增高）
        lastScrollHeight = Math.min(
            lastScrollHeight,
            container.scrollHeight - 1,
        );
        schedulePin();
    };

    const pauseFollow = (reason: string) => {
        if (stopped || pausedByUser) {
            return;
        }
        pausedByUser = true;
        scrollTopAtPause = container.scrollTop;
        if (reason.startsWith("wheel") || reason.startsWith("keydown:ArrowUp") || reason.startsWith("keydown:PageUp") || reason.startsWith("keydown:Home")) {
            resumeLockedUntil = performance.now() + 450;
        }
        if (frameRef !== null) {
            cancelAnimationFrame(frameRef);
            frameRef = null;
        }
        pinScheduled = false;
    };

    // 高度收缩时 Virtuoso / 浏览器滚动锚定会补偿 scrollTop，且可能过量补偿到远离底部；
    // 用户主动上滑会 pausedByUser，其余偏离底部的滚动视为程序触发并重新贴底。
    const handleScroll = () => {
        if (stopped || isPinningScroll) {
            return;
        }
        if (pausedByUser) {
            // 已近底，或用户明显下滚并进入「一屏内」→ 恢复跟滚
            const movedDown = container.scrollTop > scrollTopAtPause + 8;
            const oneScreen = Math.max(resumeBottomThresholdPx, container.clientHeight);
            if (isNearBottomForResume()) {
                resumeFollow("scroll-near-bottom");
                return;
            }
            if (movedDown && getDistanceToBottom() <= oneScreen) {
                resumeFollow("scroll-toward-bottom");
            }
            return;
        }
        if (isAtBottom()) {
            return;
        }
        schedulePin();
    };

    const handleUserIntent = (event: Event) => {
        if (stopped || isPinningScroll) {
            return;
        }
        // End / PageDown / ArrowDown / Space：回底/下翻意图 → 立刻恢复跟滚并贴底
        // （不要求已近底：WebView 里 End 常落在输入框，或 Virtuoso 残留距离 > 阈值）
        if (event.type === "keydown") {
            const key = (event as KeyboardEvent).key;
            if (
                key === "End"
                || key === "PageDown"
                || key === "ArrowDown"
                || key === " "
            ) {
                if (pausedByUser) {
                    resumeFollow(`keydown:${key}`, { force: true });
                } else {
                    schedulePin();
                }
                return;
            }
            // Home / PageUp / ArrowUp：暂停
            if (
                key === "Home"
                || key === "PageUp"
                || key === "ArrowUp"
            ) {
                pauseFollow(`keydown:${key}`);
            }
            return;
        }

        const isWheel = event.type === "wheel" && "deltaY" in event;
        const deltaY = isWheel ? (event as WheelEvent).deltaY : 0;

        // 滚轮向下：回底意图 → 立刻恢复并贴底（case 3 关键路径）
        if (isWheel && deltaY > 0) {
            if (pausedByUser) {
                resumeFollow("wheel-down", { force: true });
            } else {
                schedulePin();
            }
            return;
        }

        // 滚轮向上 → 暂停
        if (isWheel && deltaY < 0) {
            pauseFollow("wheel-up");
            return;
        }

        // touchstart / pointerdown：仅在已离底时暂停（避免误触）
        if (!isNearBottomForResume()) {
            pauseFollow(event.type);
        }
    };

    const resizeObserver = new ResizeObserver(() => {
        if (!shouldContinue()) {
            stop();
            return;
        }
        schedulePin();
    });

    const observed = new Set<Element>();
    const observeElement = (element: Element | null | undefined) => {
        if (!element || observed.has(element)) {
            return;
        }
        observed.add(element);
        resizeObserver.observe(element);
    };

    const observeKnownElements = () => {
        observeElement(container);
        Array.from(container.children).forEach(observeElement);
        contentElements.forEach(observeElement);
    };

    observeKnownElements();

    // Virtuoso / 异步内容插入新节点时，仅观察首批 children 会漏掉后续增高
    const mutationObserver = new MutationObserver(() => {
        if (stopped || !shouldContinue()) {
            return;
        }
        observeKnownElements();
        schedulePin();
    });
    mutationObserver.observe(container, {
        childList: true,
        subtree: true,
    });

    // scrollHeight 变化不一定触发 ResizeObserver（虚拟列表 spacer / 估高校正）；轮询兜底
    const heightPollId = window.setInterval(() => {
        if (stopped) {
            return;
        }
        if (!shouldContinue()) {
            stop();
            return;
        }
        // 暂停中若已近底（End 键等可能不触发 scroll）→ 恢复
        if (pausedByUser && isNearBottomForResume()) {
            resumeFollow("poll-near-bottom");
            return;
        }
        const nextHeight = container.scrollHeight;
        if (nextHeight === lastScrollHeight) {
            return;
        }
        if (pausedByUser) {
            // 保留 delta：不推进 lastScrollHeight，恢复后仍能看到增高
            return;
        }
        lastScrollHeight = nextHeight;
        schedulePin();
    }, 100);

    const timeoutId = window.setTimeout(() => stop(), maxDurationMs);

    function stop() {
        if (stopped) {
            return;
        }
        stopped = true;
        resizeObserver.disconnect();
        mutationObserver.disconnect();
        container.removeEventListener("scroll", handleScroll);
        USER_INTENT_EVENTS.forEach((eventName) => {
            container.removeEventListener(eventName, handleUserIntent);
        });
        if (frameRef !== null) {
            cancelAnimationFrame(frameRef);
            frameRef = null;
        }
        pinScheduled = false;
        window.clearInterval(heightPollId);
        window.clearTimeout(timeoutId);
    }

    container.addEventListener("scroll", handleScroll, { passive: true });
    USER_INTENT_EVENTS.forEach((eventName) => {
        container.addEventListener(eventName, handleUserIntent, { passive: true });
    });
    // 启动时先贴一次底；用户随后上滑仍会 pause（case 2）
    schedulePin();
    return stop;
}
