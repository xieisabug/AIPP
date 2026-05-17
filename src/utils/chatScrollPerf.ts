const DEFAULT_FRAME_BUDGET_MS = 1000 / 60;

export interface ChatScrollFrameSummary {
    sampleCount: number;
    averageFrameMs: number;
    p95FrameMs: number;
    worstFrameMs: number;
    overBudgetFrameCount: number;
    severeFrameCount: number;
    estimatedDroppedFrameCount: number;
}

export interface ChatScrollProbeOptions {
    durationMs?: number;
    includeReturnTrip?: boolean;
    settleFrameCount?: number;
}

export interface ChatScrollProbeResult extends ChatScrollFrameSummary {
    totalDurationMs: number;
    maxScrollTop: number;
    finalMaxScrollTop: number;
    minObservedMaxScrollTop: number;
    maxObservedMaxScrollTop: number;
    startScrollTop: number;
    endScrollTop: number;
    scrollEventCount: number;
    viewportSampleCount: number;
    blankViewportSampleCount: number;
    averageBlankViewportRatio: number;
    maxBlankViewportRatio: number;
    minVisibleMessageCount: number;
    warning?: string;
}

function roundToTwo(value: number): number {
    return Math.round(value * 100) / 100;
}

function getPercentile(sortedValues: number[], percentile: number): number {
    if (sortedValues.length === 0) {
        return 0;
    }

    const index = Math.min(
        sortedValues.length - 1,
        Math.max(0, Math.ceil(sortedValues.length * percentile) - 1),
    );

    return sortedValues[index];
}

export function summarizeFrameDurations(
    frameDurations: number[],
    frameBudgetMs: number = DEFAULT_FRAME_BUDGET_MS,
): ChatScrollFrameSummary {
    if (frameDurations.length === 0) {
        return {
            sampleCount: 0,
            averageFrameMs: 0,
            p95FrameMs: 0,
            worstFrameMs: 0,
            overBudgetFrameCount: 0,
            severeFrameCount: 0,
            estimatedDroppedFrameCount: 0,
        };
    }

    const sortedValues = [...frameDurations].sort((a, b) => a - b);
    const totalFrameTime = frameDurations.reduce((sum, value) => sum + value, 0);
    const severeThresholdMs = frameBudgetMs * 2;
    const estimatedDroppedFrameCount = frameDurations.reduce((sum, value) => {
        if (value <= frameBudgetMs) {
            return sum;
        }

        return sum + Math.max(0, Math.round(value / frameBudgetMs) - 1);
    }, 0);

    return {
        sampleCount: frameDurations.length,
        averageFrameMs: roundToTwo(totalFrameTime / frameDurations.length),
        p95FrameMs: roundToTwo(getPercentile(sortedValues, 0.95)),
        worstFrameMs: roundToTwo(sortedValues[sortedValues.length - 1]),
        overBudgetFrameCount: frameDurations.filter(
            (value) => value > frameBudgetMs,
        ).length,
        severeFrameCount: frameDurations.filter(
            (value) => value > severeThresholdMs,
        ).length,
        estimatedDroppedFrameCount,
    };
}

export function waitForAnimationFrames(frameCount: number = 2): Promise<void> {
    const targetCount = Math.max(0, Math.floor(frameCount));
    if (targetCount === 0) {
        return Promise.resolve();
    }

    return new Promise((resolve) => {
        let remaining = targetCount;

        const step = () => {
            remaining -= 1;
            if (remaining <= 0) {
                resolve();
                return;
            }

            requestAnimationFrame(step);
        };

        requestAnimationFrame(step);
    });
}

export function waitForCondition(
    predicate: () => boolean,
    options: {
        timeoutMs?: number;
        intervalMs?: number;
    } = {},
): Promise<void> {
    const { timeoutMs = 5000, intervalMs = 50 } = options;

    return new Promise((resolve, reject) => {
        const startedAt = performance.now();
        const timer = window.setInterval(() => {
            if (predicate()) {
                window.clearInterval(timer);
                resolve();
                return;
            }

            if (performance.now() - startedAt >= timeoutMs) {
                window.clearInterval(timer);
                reject(new Error("Timed out waiting for UI condition"));
            }
        }, intervalMs);
    });
}

function easeInOutQuad(progress: number): number {
    return progress < 0.5
        ? 2 * progress * progress
        : 1 - Math.pow(-2 * progress + 2, 2) / 2;
}

function getMergedIntervalLength(intervals: Array<[number, number]>): number {
    if (intervals.length === 0) {
        return 0;
    }

    const sortedIntervals = [...intervals].sort((a, b) => a[0] - b[0]);
    let coveredLength = 0;
    let [currentStart, currentEnd] = sortedIntervals[0];

    for (let index = 1; index < sortedIntervals.length; index += 1) {
        const [nextStart, nextEnd] = sortedIntervals[index];
        if (nextStart <= currentEnd) {
            currentEnd = Math.max(currentEnd, nextEnd);
            continue;
        }

        coveredLength += currentEnd - currentStart;
        currentStart = nextStart;
        currentEnd = nextEnd;
    }

    coveredLength += currentEnd - currentStart;
    return coveredLength;
}

function measureViewportBlankness(container: HTMLElement): {
    blankRatio: number;
    visibleMessageCount: number;
} {
    const viewportHeight = container.clientHeight;
    if (viewportHeight <= 0) {
        return {
            blankRatio: 0,
            visibleMessageCount: 0,
        };
    }

    const containerRect = container.getBoundingClientRect();
    const viewportTop = containerRect.top;
    const viewportBottom = containerRect.bottom;
    const visibleIntervals: Array<[number, number]> = [];
    let visibleMessageCount = 0;

    container.querySelectorAll<HTMLElement>("[data-message-item]").forEach((item) => {
        const rect = item.getBoundingClientRect();
        const intersectionTop = Math.max(rect.top, viewportTop);
        const intersectionBottom = Math.min(rect.bottom, viewportBottom);
        if (intersectionBottom <= intersectionTop) {
            return;
        }

        visibleMessageCount += 1;
        visibleIntervals.push([
            intersectionTop - viewportTop,
            intersectionBottom - viewportTop,
        ]);
    });

    const coveredHeight = getMergedIntervalLength(visibleIntervals);
    const blankRatio = Math.max(
        0,
        Math.min(1, 1 - coveredHeight / viewportHeight),
    );

    return {
        blankRatio,
        visibleMessageCount,
    };
}

async function runScrollPass(
    container: HTMLElement,
    fromScrollTop: number,
    toScrollTop: number,
    durationMs: number,
    frameDurations: number[],
    lastFrameAtRef: { current: number | null },
    observeMaxScrollTop: () => void,
    recordViewportSample: () => void,
): Promise<void> {
    if (durationMs <= 0 || Math.abs(toScrollTop - fromScrollTop) < 1) {
        container.scrollTop = toScrollTop;
        observeMaxScrollTop();
        recordViewportSample();
        return;
    }

    await new Promise<void>((resolve) => {
        let passStartedAt = 0;

        const step = (now: number) => {
            if (passStartedAt === 0) {
                passStartedAt = now;
            }

            if (lastFrameAtRef.current !== null) {
                frameDurations.push(now - lastFrameAtRef.current);
            }
            lastFrameAtRef.current = now;
            observeMaxScrollTop();

            const progress = Math.min(1, (now - passStartedAt) / durationMs);
            const easedProgress = easeInOutQuad(progress);
            container.scrollTop =
                fromScrollTop + (toScrollTop - fromScrollTop) * easedProgress;
            recordViewportSample();

            if (progress >= 1) {
                container.scrollTop = toScrollTop;
                recordViewportSample();
                resolve();
                return;
            }

            requestAnimationFrame(step);
        };

        requestAnimationFrame(step);
    });
}

export async function runScrollPerformanceProbe(
    container: HTMLElement,
    options: ChatScrollProbeOptions = {},
): Promise<ChatScrollProbeResult> {
    const {
        durationMs = 2200,
        includeReturnTrip = true,
        settleFrameCount = 4,
    } = options;

    await waitForAnimationFrames(settleFrameCount);

    const startScrollTop = container.scrollTop;
    const maxScrollTop = Math.max(
        0,
        container.scrollHeight - container.clientHeight,
    );
    let minObservedMaxScrollTop = maxScrollTop;
    let maxObservedMaxScrollTop = maxScrollTop;
    const observeMaxScrollTop = () => {
        const observedMaxScrollTop = Math.max(
            0,
            container.scrollHeight - container.clientHeight,
        );
        minObservedMaxScrollTop = Math.min(
            minObservedMaxScrollTop,
            observedMaxScrollTop,
        );
        maxObservedMaxScrollTop = Math.max(
            maxObservedMaxScrollTop,
            observedMaxScrollTop,
        );
    };

    if (maxScrollTop <= 0) {
        return {
            ...summarizeFrameDurations([]),
            totalDurationMs: 0,
            maxScrollTop,
            finalMaxScrollTop: maxScrollTop,
            minObservedMaxScrollTop: maxScrollTop,
            maxObservedMaxScrollTop: maxScrollTop,
            startScrollTop,
            endScrollTop: container.scrollTop,
            scrollEventCount: 0,
            viewportSampleCount: 0,
            blankViewportSampleCount: 0,
            averageBlankViewportRatio: 0,
            maxBlankViewportRatio: 0,
            minVisibleMessageCount: 0,
            warning: "Scroll container does not overflow; no scroll probe was run.",
        };
    }

    const frameDurations: number[] = [];
    const blankViewportRatios: number[] = [];
    let blankViewportSampleCount = 0;
    let minVisibleMessageCount = Number.POSITIVE_INFINITY;
    let scrollEventCount = 0;
    const handleScroll = () => {
        scrollEventCount += 1;
        observeMaxScrollTop();
    };
    const recordViewportSample = () => {
        const sample = measureViewportBlankness(container);
        blankViewportRatios.push(sample.blankRatio);
        if (sample.blankRatio >= 0.98) {
            blankViewportSampleCount += 1;
        }
        minVisibleMessageCount = Math.min(
            minVisibleMessageCount,
            sample.visibleMessageCount,
        );
    };

    container.addEventListener("scroll", handleScroll, { passive: true });

    const lastFrameAtRef = { current: null as number | null };
    const startedAt = performance.now();

    try {
        container.scrollTop = 0;
        await waitForAnimationFrames(2);

        await runScrollPass(
            container,
            0,
            maxScrollTop,
            durationMs,
            frameDurations,
            lastFrameAtRef,
            observeMaxScrollTop,
            recordViewportSample,
        );

        if (includeReturnTrip) {
            await runScrollPass(
                container,
                maxScrollTop,
                0,
                durationMs,
                frameDurations,
                lastFrameAtRef,
                observeMaxScrollTop,
                recordViewportSample,
            );
        }
    } finally {
        container.removeEventListener("scroll", handleScroll);
    }

    await waitForAnimationFrames(2);
    observeMaxScrollTop();
    const finalMaxScrollTop = Math.max(
        0,
        container.scrollHeight - container.clientHeight,
    );
    const viewportSampleCount = blankViewportRatios.length;
    const averageBlankViewportRatio = viewportSampleCount > 0
        ? blankViewportRatios.reduce((sum, value) => sum + value, 0)
            / viewportSampleCount
        : 0;
    const maxBlankViewportRatio = viewportSampleCount > 0
        ? Math.max(...blankViewportRatios)
        : 0;

    return {
        ...summarizeFrameDurations(frameDurations),
        totalDurationMs: roundToTwo(performance.now() - startedAt),
        maxScrollTop: roundToTwo(maxScrollTop),
        finalMaxScrollTop: roundToTwo(finalMaxScrollTop),
        minObservedMaxScrollTop: roundToTwo(minObservedMaxScrollTop),
        maxObservedMaxScrollTop: roundToTwo(maxObservedMaxScrollTop),
        startScrollTop: roundToTwo(startScrollTop),
        endScrollTop: roundToTwo(container.scrollTop),
        scrollEventCount,
        viewportSampleCount,
        blankViewportSampleCount,
        averageBlankViewportRatio: roundToTwo(averageBlankViewportRatio),
        maxBlankViewportRatio: roundToTwo(maxBlankViewportRatio),
        minVisibleMessageCount:
            minVisibleMessageCount === Number.POSITIVE_INFINITY
                ? 0
                : minVisibleMessageCount,
    };
}
