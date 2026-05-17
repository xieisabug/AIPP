import React, {
    useCallback,
    useEffect,
    useLayoutEffect,
    useMemo,
    useRef,
    useState,
} from "react";

import { applyScrollHighlight } from "./scrollHighlight";
import {
    VIRTUAL_OVERSCAN_PX,
    buildVirtualizedLayout,
    findVisibleRange,
    getCenteredScrollTopForIndex,
} from "./virtualizedMessageListLayout";
import {
    useMessageListElements,
    type UseMessageListElementsProps,
} from "./useMessageListElements";

declare global {
    interface Window {
        __AIPP_CHAT_PERF_CAPTURE__?: {
            recordVirtualRowHeight?: (key: string, height: number) => void;
            resetVirtualRowHeightDrift?: () => void;
            getVirtualRowHeightDrift?: () => Array<{
                key: string;
                changeCount: number;
                minHeight: number;
                maxHeight: number;
                delta: number;
                lastHeight: number;
            }>;
        };
    }
}

interface VirtualizedMessageListProps extends UseMessageListElementsProps {
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    pendingScrollMessageId: number | null;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    smartScroll: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
}

const MAX_SCROLL_HIGHLIGHT_ATTEMPTS = 60;
const NEAR_BOTTOM_PIN_PX = 96;
const HEIGHT_SHRINK_DEFER_MS = 160;

interface VirtualizedRowProps {
    itemKey: string;
    top: number;
    onHeightChange: (key: string, height: number) => void;
    children: React.ReactNode;
}

const VirtualizedRow = React.memo(
    ({ itemKey, top, onHeightChange, children }: VirtualizedRowProps) => {
        const rowRef = useRef<HTMLDivElement | null>(null);

        useEffect(() => {
            const element = rowRef.current;
            if (!element) {
                return;
            }

            const reportHeight = () => {
                const nextHeight = element.offsetHeight;
                if (nextHeight > 0) {
                    onHeightChange(itemKey, nextHeight);
                }
            };

            reportHeight();

            const observer = new ResizeObserver(() => {
                reportHeight();
            });
            observer.observe(element);

            return () => {
                observer.disconnect();
            };
        }, [itemKey, onHeightChange]);

        return (
            <div
                ref={rowRef}
                style={{
                    position: "absolute",
                    top,
                    left: 0,
                    right: 0,
                }}
            >
                {children}
            </div>
        );
    },
);

VirtualizedRow.displayName = "VirtualizedRow";

const VirtualizedMessageList: React.FC<VirtualizedMessageListProps> = ({
    scrollContainerRef,
    pendingScrollMessageId,
    clearPendingScrollMessageId,
    setShiningMessageIds,
    onScrollStateChange,
    smartScroll,
    ...messageListProps
}) => {
    const { renderItems } = useMessageListElements(messageListProps);
    const firstLiveIndex = useMemo(
        () =>
            renderItems.findIndex(
                (item) => item.virtualizationMode === "live",
            ),
        [renderItems],
    );
    const virtualizedItems = useMemo(
        () =>
            firstLiveIndex >= 0
                ? renderItems.slice(0, firstLiveIndex)
                : renderItems,
        [firstLiveIndex, renderItems],
    );
    const liveItems = useMemo(
        () =>
            firstLiveIndex >= 0 ? renderItems.slice(firstLiveIndex) : [],
        [firstLiveIndex, renderItems],
    );
    const tailItem = useMemo(
        () =>
            liveItems.find((item) => item.key === "last-reply-container")
            ?? renderItems.find((item) => item.key === "last-reply-container")
            ?? null,
        [liveItems, renderItems],
    );
    const [scrollTop, setScrollTop] = useState(0);
    const [viewportHeight, setViewportHeight] = useState(0);
    const [measuredHeights, setMeasuredHeights] = useState<Record<string, number>>(
        {},
    );
    const measuredHeightsRef = useRef<Record<string, number>>({});
    const pendingMeasuredHeightsRef = useRef<Record<string, number>>({});
    const flushMeasuredHeightsRef = useRef<(() => void) | null>(null);
    const previousLayoutRef = useRef<ReturnType<typeof buildVirtualizedLayout> | null>(
        null,
    );
    const previousRenderKeysRef = useRef<string[]>([]);
    const lastKnownScrollTopRef = useRef(0);
    const userMovedAwayFromBottomRef = useRef(false);
    const lastScrollActivityAtRef = useRef<number | null>(null);
    const lastTouchClientYRef = useRef<number | null>(null);
    const scrollMetricsFrameRef = useRef<number | null>(null);
    const measuredHeightFlushFrameRef = useRef<number | null>(null);
    const shrinkFlushTimeoutRef = useRef<number | null>(null);

    const layout = useMemo(
        () => buildVirtualizedLayout(virtualizedItems, measuredHeights),
        [virtualizedItems, measuredHeights],
    );
    const overscanPx = useMemo(
        () => Math.max(VIRTUAL_OVERSCAN_PX, viewportHeight * 6),
        [viewportHeight],
    );
    const visibleRange = useMemo(
        () => findVisibleRange(layout, scrollTop, viewportHeight, overscanPx),
        [layout, overscanPx, scrollTop, viewportHeight],
    );
    const visibleItems = useMemo(() => {
        return virtualizedItems.slice(
            visibleRange.startIndex,
            visibleRange.endIndex,
        );
    }, [virtualizedItems, visibleRange.endIndex, visibleRange.startIndex]);

    useEffect(() => {
        measuredHeightsRef.current = measuredHeights;
    }, [measuredHeights]);

    const syncScrollMetrics = useCallback(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        lastScrollActivityAtRef.current = performance.now();

        const nextScrollTop = container.scrollTop;
        const previousScrollTop = lastKnownScrollTopRef.current;
        if (nextScrollTop < previousScrollTop - 1) {
            userMovedAwayFromBottomRef.current = true;
        }
        const distanceToBottom = Math.max(
            0,
            container.scrollHeight - nextScrollTop - container.clientHeight,
        );
        if (distanceToBottom <= 10) {
            userMovedAwayFromBottomRef.current = false;
        }
        lastKnownScrollTopRef.current = nextScrollTop;

        onScrollStateChange?.(container);
        setScrollTop(nextScrollTop);
        setViewportHeight(container.clientHeight);
    }, [onScrollStateChange, scrollContainerRef]);

    const scheduleScrollMetricsUpdate = useCallback(() => {
        if (scrollMetricsFrameRef.current !== null) {
            return;
        }

        scrollMetricsFrameRef.current = requestAnimationFrame(() => {
            scrollMetricsFrameRef.current = null;
            syncScrollMetrics();
        });
    }, [syncScrollMetrics]);

    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        syncScrollMetrics();
        const handleScroll = () => {
            scheduleScrollMetricsUpdate();
        };
        const handleWheel = (event: WheelEvent) => {
            if (event.deltaY < 0) {
                userMovedAwayFromBottomRef.current = true;
            }
        };
        const handleTouchStart = (event: TouchEvent) => {
            lastTouchClientYRef.current = event.touches[0]?.clientY ?? null;
        };
        const handleTouchMove = (event: TouchEvent) => {
            const nextClientY = event.touches[0]?.clientY ?? null;
            const previousClientY = lastTouchClientYRef.current;
            if (
                nextClientY !== null
                && previousClientY !== null
                && nextClientY > previousClientY + 1
            ) {
                userMovedAwayFromBottomRef.current = true;
            }
            lastTouchClientYRef.current = nextClientY;
        };

        container.addEventListener("scroll", handleScroll, { passive: true });
        container.addEventListener("wheel", handleWheel, { passive: true });
        container.addEventListener("touchstart", handleTouchStart, { passive: true });
        container.addEventListener("touchmove", handleTouchMove, { passive: true });
        const resizeObserver = new ResizeObserver(() => {
            scheduleScrollMetricsUpdate();
        });
        resizeObserver.observe(container);

        return () => {
            if (scrollMetricsFrameRef.current !== null) {
                cancelAnimationFrame(scrollMetricsFrameRef.current);
                scrollMetricsFrameRef.current = null;
            }
            container.removeEventListener("scroll", handleScroll);
            container.removeEventListener("wheel", handleWheel);
            container.removeEventListener("touchstart", handleTouchStart);
            container.removeEventListener("touchmove", handleTouchMove);
            resizeObserver.disconnect();
        };
    }, [scheduleScrollMetricsUpdate, scrollContainerRef, syncScrollMetrics]);

    const clearShrinkFlushTimeout = useCallback(() => {
        if (shrinkFlushTimeoutRef.current !== null) {
            window.clearTimeout(shrinkFlushTimeoutRef.current);
            shrinkFlushTimeoutRef.current = null;
        }
    }, []);

    const scheduleShrinkFlushAfterIdle = useCallback(() => {
        clearShrinkFlushTimeout();
        shrinkFlushTimeoutRef.current = window.setTimeout(() => {
            shrinkFlushTimeoutRef.current = null;
            flushMeasuredHeightsRef.current?.();
        }, HEIGHT_SHRINK_DEFER_MS);
    }, [clearShrinkFlushTimeout]);

    const isScrollActive = useCallback(() => {
        return (
            lastScrollActivityAtRef.current !== null
            && performance.now() - lastScrollActivityAtRef.current
                < HEIGHT_SHRINK_DEFER_MS
        );
    }, []);

    const flushMeasuredHeights = useCallback(() => {
        measuredHeightFlushFrameRef.current = null;
        const pendingEntries = Object.entries(pendingMeasuredHeightsRef.current);
        if (pendingEntries.length === 0) {
            return;
        }

        pendingMeasuredHeightsRef.current = {};
        const deferredShrinks: Record<string, number> = {};
        setMeasuredHeights((prev) => {
            let changed = false;
            const next = { ...prev };

            pendingEntries.forEach(([key, height]) => {
                const currentHeight = next[key];
                if (currentHeight === height) {
                    return;
                }
                if (
                    typeof currentHeight === "number"
                    && height < currentHeight
                    && isScrollActive()
                ) {
                    deferredShrinks[key] = height;
                    return;
                }
                next[key] = height;
                changed = true;
            });

            if (!changed) {
                return prev;
            }

            measuredHeightsRef.current = next;
            return next;
        });

        const deferredShrinkEntries = Object.entries(deferredShrinks);
        if (deferredShrinkEntries.length > 0) {
            deferredShrinkEntries.forEach(([key, height]) => {
                pendingMeasuredHeightsRef.current[key] = height;
            });
            scheduleShrinkFlushAfterIdle();
        }
    }, [isScrollActive, scheduleShrinkFlushAfterIdle]);

    flushMeasuredHeightsRef.current = flushMeasuredHeights;

    const scheduleMeasuredHeightFlush = useCallback(() => {
        if (measuredHeightFlushFrameRef.current !== null) {
            return;
        }

        measuredHeightFlushFrameRef.current = requestAnimationFrame(() => {
            flushMeasuredHeights();
        });
    }, [flushMeasuredHeights]);

    const handleHeightChange = useCallback((key: string, height: number) => {
        window.__AIPP_CHAT_PERF_CAPTURE__?.recordVirtualRowHeight?.(key, height);
        const currentHeight =
            pendingMeasuredHeightsRef.current[key] ?? measuredHeightsRef.current[key];
        if (currentHeight === height) {
            return;
        }

        pendingMeasuredHeightsRef.current[key] = height;
        scheduleMeasuredHeightFlush();
    }, [scheduleMeasuredHeightFlush]);

    useEffect(() => {
        return () => {
            if (measuredHeightFlushFrameRef.current !== null) {
                cancelAnimationFrame(measuredHeightFlushFrameRef.current);
                measuredHeightFlushFrameRef.current = null;
            }
            clearShrinkFlushTimeout();
            pendingMeasuredHeightsRef.current = {};
        };
    }, [clearShrinkFlushTimeout]);

    useLayoutEffect(() => {
        const container = scrollContainerRef.current;
        const previousLayout = previousLayoutRef.current;
        const previousRenderKeys = previousRenderKeysRef.current;

        previousLayoutRef.current = layout;
        previousRenderKeysRef.current = virtualizedItems.map((item) => item.key);

        if (!container || !previousLayout) {
            return;
        }
        if (pendingScrollMessageId !== null) {
            return;
        }

        const currentScrollTop = container.scrollTop;
        const currentViewportHeight = container.clientHeight;
        const previousVirtualMaxScrollTop = Math.max(
            0,
            previousLayout.totalHeight - currentViewportHeight,
        );
        const previousDistanceToBottom = Math.max(
            0,
            previousVirtualMaxScrollTop - currentScrollTop,
        );
        const currentMaxScrollTop = Math.max(
            0,
            container.scrollHeight - currentViewportHeight,
        );
        const currentDistanceToBottom = Math.max(
            0,
            currentMaxScrollTop - currentScrollTop,
        );
        const shouldPinBottom =
            !userMovedAwayFromBottomRef.current
            && (
                previousDistanceToBottom <= 10
                || currentDistanceToBottom <= NEAR_BOTTOM_PIN_PX
            );

        if (shouldPinBottom) {
            userMovedAwayFromBottomRef.current = false;
            const nextMaxScrollTop = currentMaxScrollTop;
            if (Math.abs(nextMaxScrollTop - currentScrollTop) > 1) {
                container.scrollTop = nextMaxScrollTop;
                syncScrollMetrics();
            }
            return;
        }

        const anchorRange = findVisibleRange(
            previousLayout,
            currentScrollTop,
            currentViewportHeight,
            0,
        );
        const anchorIndex = anchorRange.startIndex;
        const anchorKey = previousRenderKeys[anchorIndex];
        if (!anchorKey) {
            return;
        }

        const nextAnchorIndex = virtualizedItems.findIndex(
            (item) => item.key === anchorKey,
        );
        if (nextAnchorIndex < 0) {
            return;
        }

        const offsetWithinViewport =
            (previousLayout.tops[anchorIndex] ?? 0) - currentScrollTop;
        const nextScrollTop =
            (layout.tops[nextAnchorIndex] ?? 0) - offsetWithinViewport;
        if (Math.abs(nextScrollTop - currentScrollTop) > 1) {
            container.scrollTop = Math.max(0, nextScrollTop);
            syncScrollMetrics();
        }
    }, [
        layout,
        pendingScrollMessageId,
        virtualizedItems,
        scrollContainerRef,
        syncScrollMetrics,
    ]);

    useEffect(() => {
        if (pendingScrollMessageId === null) {
            return;
        }

        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        const targetIsTail =
            !!tailItem
            && (
                tailItem.messageId === pendingScrollMessageId
                || tailItem.messageIds?.includes(pendingScrollMessageId)
            );
        const targetIndex = virtualizedItems.findIndex(
            (item) =>
                item.messageId === pendingScrollMessageId
                || item.messageIds?.includes(pendingScrollMessageId),
        );
        if (targetIsTail) {
            const existingTarget = container.querySelector(
                `[data-message-id='${pendingScrollMessageId}']`,
            ) as HTMLElement | null;
            if (existingTarget) {
                existingTarget.scrollIntoView({
                    block: "center",
                    behavior: "smooth",
                });
            } else {
                container.scrollTo({
                    top: Math.max(
                        0,
                        container.scrollHeight - container.clientHeight,
                    ),
                    behavior: "smooth",
                });
            }

            let attempts = 0;
            const tryHighlightTail = () => {
                const target = container.querySelector(
                    `[data-message-id='${pendingScrollMessageId}']`,
                ) as HTMLElement | null;
                if (target) {
                    applyScrollHighlight({
                        target,
                        messageId: pendingScrollMessageId,
                        setShiningMessageIds,
                        clearPendingScrollMessageId,
                    });
                    return;
                }

                attempts += 1;
                if (attempts >= MAX_SCROLL_HIGHLIGHT_ATTEMPTS) {
                    clearPendingScrollMessageId(null);
                    return;
                }

                requestAnimationFrame(tryHighlightTail);
            };

            requestAnimationFrame(tryHighlightTail);
            return;
        }
        if (targetIndex < 0) {
            clearPendingScrollMessageId(null);
            return;
        }

        const targetScrollTop = getCenteredScrollTopForIndex(
            layout,
            viewportHeight || container.clientHeight,
            targetIndex,
        );
        container.scrollTo({
            top: targetScrollTop,
            behavior: "smooth",
        });

        let attempts = 0;
        const tryHighlight = () => {
            const target = container.querySelector(
                `[data-message-id='${pendingScrollMessageId}']`,
            ) as HTMLElement | null;
            if (target) {
                applyScrollHighlight({
                    target,
                    messageId: pendingScrollMessageId,
                    setShiningMessageIds,
                    clearPendingScrollMessageId,
                });
                return;
            }

            attempts += 1;
            if (attempts >= MAX_SCROLL_HIGHLIGHT_ATTEMPTS) {
                clearPendingScrollMessageId(null);
                return;
            }

            requestAnimationFrame(tryHighlight);
        };

        requestAnimationFrame(tryHighlight);
    }, [
        clearPendingScrollMessageId,
        layout,
        pendingScrollMessageId,
        tailItem,
        virtualizedItems,
        scrollContainerRef,
        setShiningMessageIds,
        viewportHeight,
    ]);

    return (
        <div
            style={{
                position: "relative",
                height:
                    layout.totalHeight > 0
                        ? layout.totalHeight
                        : liveItems.length > 0
                          ? undefined
                          : 1,
                minHeight:
                    layout.totalHeight > 0
                        ? layout.totalHeight
                        : 1,
            }}
        >
            {visibleItems.map((item, relativeIndex) => {
                const absoluteIndex = visibleRange.startIndex + relativeIndex;
                return (
                    <VirtualizedRow
                        key={item.key}
                        itemKey={item.key}
                        top={layout.tops[absoluteIndex] ?? 0}
                        onHeightChange={handleHeightChange}
                    >
                        {item.element}
                    </VirtualizedRow>
                );
            })}
            {liveItems.length > 0 && (
                <div
                    style={{
                        position: layout.totalHeight > 0 ? "absolute" : "relative",
                        top: layout.totalHeight > 0 ? layout.totalHeight + 16 : undefined,
                        left: 0,
                        right: 0,
                    }}
                    className="flex flex-col gap-4"
                >
                    {liveItems.map((item) => (
                        <React.Fragment key={item.key}>{item.element}</React.Fragment>
                    ))}
                </div>
            )}
        </div>
    );
};

export default React.memo(VirtualizedMessageList);
