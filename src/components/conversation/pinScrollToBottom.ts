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
    /**
     * 用户滚轮上滑或拖动滚动条时结束钉底。
     * 调用方应记下「用户已接手」，避免效果重跑后又把视口钉回底部。
     */
    onUserRelease?: () => void;
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

const SCROLLBAR_GUTTER_MIN_PX = 16;

/** 指针落在容器右缘滚动条上。内容区点击的坐标在这个窄条之外。 */
export function isScrollbarPointerEvent(
    container: HTMLElement,
    event: Event,
): boolean {
    if (event.type !== "pointerdown" && event.type !== "mousedown") {
        return false;
    }
    if (!("clientX" in event) || typeof (event as MouseEvent).clientX !== "number") {
        return false;
    }
    const clientX = (event as MouseEvent).clientX;
    if (!Number.isFinite(clientX)) {
        return false;
    }
    const rect = container.getBoundingClientRect();
    const measuredGutter = container.offsetWidth - container.clientWidth;
    const gutter = Math.max(measuredGutter, SCROLLBAR_GUTTER_MIN_PX);
    return clientX >= rect.right - gutter && clientX <= rect.right + 1;
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
    onUserRelease,
}: PinScrollToBottomOptions): () => void {
    let elapsedFrames = 0;
    let stableFrames = 0;
    let lastMaxScrollTop: number | null = null;
    let isPinningScroll = false;
    let frameRef: number | null = null;
    let failsafeTimerId: number | null = null;
    let completed = false;
    let lastScrollTop = container.scrollTop;
    let lastScrollHeight = container.scrollHeight;

    const pinToBottom = () => {
        if (completed || !shouldContinue()) {
            return;
        }

        isPinningScroll = true;
        container.scrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );
        isPinningScroll = false;
        lastScrollTop = container.scrollTop;
        lastScrollHeight = container.scrollHeight;
        onScrollStateChange?.(container);
    };

    const releaseToUser = () => {
        if (completed) {
            return;
        }
        completed = true;
        cleanup();
        onUserRelease?.();
    };

    const handlePinnedScroll = () => {
        if (completed || isPinningScroll || !shouldContinue()) {
            return;
        }

        const movedUp = container.scrollTop < lastScrollTop - 1;
        const heightUnchanged = Math.abs(container.scrollHeight - lastScrollHeight) <= 1;
        // 滚动条拖动通常只产生 scroll、没有指针坐标。高度没变却往上走，就是用户在翻。
        if (movedUp && heightUnchanged) {
            releaseToUser();
            return;
        }

        lastScrollTop = container.scrollTop;
        lastScrollHeight = container.scrollHeight;
        pinToBottom();
    };

    const handleUserIntent = (event: Event) => {
        if (completed || isPinningScroll) {
            return;
        }
        if (event.type === "wheel" && "deltaY" in event && (event as WheelEvent).deltaY < 0) {
            releaseToUser();
            return;
        }
        if (
            (event.type === "pointerdown" || event.type === "mousedown")
            && isScrollbarPointerEvent(container, event)
        ) {
            releaseToUser();
        }
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
        container.removeEventListener("wheel", handleUserIntent);
        container.removeEventListener("pointerdown", handleUserIntent);
        container.removeEventListener("mousedown", handleUserIntent);
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
        if (completed || !shouldContinue()) {
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
    container.addEventListener("wheel", handleUserIntent, { passive: true });
    container.addEventListener("pointerdown", handleUserIntent, { passive: true });
    container.addEventListener("mousedown", handleUserIntent, { passive: true });
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
    /** 用户上滑或拖动滚动条后暂停。调用方可据此避免效果重跑时重新跟滚。 */
    onUserPause?: () => void;
    onUserResume?: () => void;
}

const DEFAULT_FOLLOW_MAX_DURATION_MS = 10_000;
const DEFAULT_FOLLOW_BOTTOM_THRESHOLD_PX = 10;
/** 恢复跟滚用更宽阈值：贴底后 Virtuoso spacer/footer 常让 distance 卡在 10~60px */
const DEFAULT_RESUME_BOTTOM_THRESHOLD_PX = 240;
const USER_INTENT_EVENTS = ["wheel", "touchstart", "pointerdown", "mousedown", "keydown"] as const;

/**
 * 初始贴底结束后，虚拟列表实测高度与估算不一致会持续收缩/增长，
 * 异步内容（代码高亮、图片、富文本预览）也可能继续长高。
 * 用户滚轮向上、拖动滚动条或触摸离底后暂停跟滚。
 * 恢复只发生在滚轮向下、End 一类回底键，或视口确实回到底部。
 */
export function followScrollContainerBottom({
    container,
    contentElements = [],
    onScrollStateChange,
    shouldContinue,
    maxDurationMs = DEFAULT_FOLLOW_MAX_DURATION_MS,
    bottomThresholdPx = DEFAULT_FOLLOW_BOTTOM_THRESHOLD_PX,
    resumeBottomThresholdPx = DEFAULT_RESUME_BOTTOM_THRESHOLD_PX,
    onUserPause,
    onUserResume,
}: FollowScrollToBottomOptions): () => void {
    let stopped = false;
    /** 用户上滑后暂停跟滚；贴底后自动恢复（与 useScrollManagement.isUserScrolledUp 对齐） */
    let pausedByUser = false;
    let scrollbarDragging = false;
    let isPinningScroll = false;
    /** rAF id；用 pinScheduled 判断是否已排队，避免同步 rAF stub 把 id 赋回后永久卡住 */
    let frameRef: number | null = null;
    let pinScheduled = false;
    let lastScrollHeight = container.scrollHeight;
    let lastScrollTop = container.scrollTop;
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
            lastScrollTop = container.scrollTop;
            lastScrollHeight = container.scrollHeight;
            return;
        }

        isPinningScroll = true;
        container.scrollTop = maxScrollTop;
        isPinningScroll = false;
        lastScrollTop = container.scrollTop;
        lastScrollHeight = container.scrollHeight;
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

    const resumeFollow = (_reason: string, opts?: { force?: boolean }) => {
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
        onUserResume?.();
        schedulePin();
    };

    const pauseFollow = (reason: string) => {
        if (stopped || pausedByUser) {
            return;
        }
        pausedByUser = true;
        scrollTopAtPause = container.scrollTop;
        if (
            reason.startsWith("wheel")
            || reason.startsWith("keydown:ArrowUp")
            || reason.startsWith("keydown:PageUp")
            || reason.startsWith("keydown:Home")
            || reason === "user-scroll"
            || reason === "scrollbar"
        ) {
            resumeLockedUntil = performance.now() + 450;
        }
        if (frameRef !== null) {
            cancelAnimationFrame(frameRef);
            frameRef = null;
        }
        pinScheduled = false;
        onUserPause?.();
    };

    // 高度收缩时 Virtuoso / 浏览器滚动锚定会补偿 scrollTop，且可能过量补偿到远离底部。
    // 高度没变却往上走是滚动条或键盘翻页，必须暂停；其余偏离视为程序触发并重新贴底。
    const handleScroll = () => {
        if (stopped || isPinningScroll) {
            return;
        }

        const movedUp = container.scrollTop < lastScrollTop - 1;
        const heightUnchanged = Math.abs(container.scrollHeight - lastScrollHeight) <= 1;
        lastScrollTop = container.scrollTop;

        if (!pausedByUser && heightUnchanged && movedUp) {
            pauseFollow("user-scroll");
            return;
        }

        if (pausedByUser) {
            // 拖动滚动条期间，或这次滚动仍在继续上移：即使还在宽松阈值内也不恢复。
            if (scrollbarDragging || container.scrollTop < scrollTopAtPause - 1) {
                scrollTopAtPause = container.scrollTop;
                return;
            }
            // 仅真正接近底部时恢复跟滚。
            // 旧逻辑「下滚且距底 ≤ 一屏」在发消息后上滑再轻微下滚时会误贴底拽走。
            if (isNearBottomForResume()) {
                resumeFollow("scroll-near-bottom");
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
        // End：明确回底。PageDown / ArrowDown / Space：仅在已接近底部时恢复跟滚，
        // 避免中段下翻被当成贴底意图拽走（与 wheel-down 对齐）。
        if (event.type === "keydown") {
            const key = (event as KeyboardEvent).key;
            if (key === "End") {
                if (pausedByUser) {
                    resumeFollow(`keydown:${key}`, { force: true });
                } else {
                    schedulePin();
                }
                return;
            }
            if (
                key === "PageDown"
                || key === "ArrowDown"
                || key === " "
            ) {
                if (pausedByUser) {
                    if (isNearBottomForResume()) {
                        resumeFollow(`keydown:${key}`, { force: true });
                    }
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

        // 滚轮向下：仅在接近底部时恢复跟滚；中段下滚保持暂停，
        // 等 scroll 把视口带进 resume 阈值后再恢复（发消息后上滑再轻下滚不得贴底）。
        if (isWheel && deltaY > 0) {
            if (pausedByUser) {
                if (isNearBottomForResume()) {
                    resumeFollow("wheel-down", { force: true });
                }
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

        // 滚动条按下：即使人还在底部也暂停。内容区点击不在这条窄边上。
        if (
            (event.type === "pointerdown" || event.type === "mousedown")
            && isScrollbarPointerEvent(container, event)
        ) {
            scrollbarDragging = true;
            pauseFollow("scrollbar");
            return;
        }

        // touchstart / 内容区 pointerdown：仅在已离底时暂停（避免误触）
        if (!isNearBottomForResume()) {
            pauseFollow(event.type);
        }
    };

    const handleScrollbarRelease = () => {
        if (!scrollbarDragging) {
            return;
        }
        scrollbarDragging = false;
        if (pausedByUser && isAtBottom()) {
            resumeFollow("scrollbar-release", { force: true });
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
        // 暂停中若确实贴底（End 键等可能不触发 scroll）→ 恢复。
        // 拖动滚动条时人还在往上走，不能因为离底不到宽松阈值就恢复。
        if (pausedByUser && !scrollbarDragging && isAtBottom()) {
            resumeFollow("poll-at-bottom");
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
        window.removeEventListener("pointerup", handleScrollbarRelease);
        window.removeEventListener("pointercancel", handleScrollbarRelease);
        window.removeEventListener("mouseup", handleScrollbarRelease);
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
    window.addEventListener("pointerup", handleScrollbarRelease);
    window.addEventListener("pointercancel", handleScrollbarRelease);
    window.addEventListener("mouseup", handleScrollbarRelease);
    // 启动时先贴一次底；用户随后上滑仍会 pause（case 2）
    schedulePin();
    return stop;
}
