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
    buildVirtualizedLayout,
    findVisibleRange,
    getCenteredScrollTopForIndex,
} from "./virtualizedMessageListLayout";
import {
    useMessageListElements,
    type UseMessageListElementsProps,
} from "./useMessageListElements";

interface VirtualizedMessageListProps extends UseMessageListElementsProps {
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    pendingScrollMessageId: number | null;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    smartScroll: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
}

const MAX_SCROLL_HIGHLIGHT_ATTEMPTS = 60;
const NEAR_BOTTOM_PIN_PX = 96;

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
    smartScroll,
    ...messageListProps
}) => {
    const { renderItems } = useMessageListElements(messageListProps);
    const [scrollTop, setScrollTop] = useState(0);
    const [viewportHeight, setViewportHeight] = useState(0);
    const [measuredHeights, setMeasuredHeights] = useState<Record<string, number>>(
        {},
    );
    const previousLayoutRef = useRef<ReturnType<typeof buildVirtualizedLayout> | null>(
        null,
    );
    const previousRenderKeysRef = useRef<string[]>([]);
    const lastKnownScrollTopRef = useRef(0);
    const userMovedAwayFromBottomRef = useRef(false);

    const layout = useMemo(
        () => buildVirtualizedLayout(renderItems, measuredHeights),
        [renderItems, measuredHeights],
    );
    const visibleRange = useMemo(
        () => findVisibleRange(layout, scrollTop, viewportHeight),
        [layout, scrollTop, viewportHeight],
    );
    const visibleItems = useMemo(() => {
        return renderItems.slice(visibleRange.startIndex, visibleRange.endIndex);
    }, [renderItems, visibleRange.endIndex, visibleRange.startIndex]);

    const updateScrollMetrics = useCallback(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

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

        setScrollTop(nextScrollTop);
        setViewportHeight(container.clientHeight);
    }, [scrollContainerRef]);

    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        updateScrollMetrics();
        const handleScroll = () => {
            updateScrollMetrics();
        };

        container.addEventListener("scroll", handleScroll, { passive: true });
        const resizeObserver = new ResizeObserver(() => {
            updateScrollMetrics();
        });
        resizeObserver.observe(container);

        return () => {
            container.removeEventListener("scroll", handleScroll);
            resizeObserver.disconnect();
        };
    }, [scrollContainerRef, updateScrollMetrics]);

    const handleHeightChange = useCallback((key: string, height: number) => {
        setMeasuredHeights((prev) => {
            if (prev[key] === height) {
                return prev;
            }
            return {
                ...prev,
                [key]: height,
            };
        });
    }, []);

    useLayoutEffect(() => {
        const container = scrollContainerRef.current;
        const previousLayout = previousLayoutRef.current;
        const previousRenderKeys = previousRenderKeysRef.current;

        previousLayoutRef.current = layout;
        previousRenderKeysRef.current = renderItems.map((item) => item.key);

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
            ((previousDistanceToBottom <= 10
                && !userMovedAwayFromBottomRef.current)
                || currentDistanceToBottom <= NEAR_BOTTOM_PIN_PX);

        if (shouldPinBottom) {
            userMovedAwayFromBottomRef.current = false;
            const nextMaxScrollTop = currentMaxScrollTop;
            if (Math.abs(nextMaxScrollTop - currentScrollTop) > 1) {
                container.scrollTop = nextMaxScrollTop;
                updateScrollMetrics();
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

        const nextAnchorIndex = renderItems.findIndex(
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
            updateScrollMetrics();
        }
    }, [
        layout,
        pendingScrollMessageId,
        renderItems,
        scrollContainerRef,
        updateScrollMetrics,
    ]);

    useEffect(() => {
        if (pendingScrollMessageId === null) {
            return;
        }

        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        const targetIndex = renderItems.findIndex(
            (item) =>
                item.messageId === pendingScrollMessageId
                || item.messageIds?.includes(pendingScrollMessageId),
        );
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
        renderItems,
        scrollContainerRef,
        setShiningMessageIds,
        viewportHeight,
    ]);

    return (
        <div
            style={{
                position: "relative",
                height: layout.totalHeight,
                minHeight: layout.totalHeight > 0 ? layout.totalHeight : 1,
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
        </div>
    );
};

export default React.memo(VirtualizedMessageList);
