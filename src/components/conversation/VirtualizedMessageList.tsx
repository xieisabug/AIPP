import React, {
    useCallback,
    useEffect,
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

        setScrollTop(container.scrollTop);
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

    useEffect(() => {
        if (pendingScrollMessageId !== null) {
            return;
        }

        requestAnimationFrame(() => {
            smartScroll();
        });
    }, [layout.totalHeight, pendingScrollMessageId, smartScroll]);

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
