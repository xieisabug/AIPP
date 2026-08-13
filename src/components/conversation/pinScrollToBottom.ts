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
