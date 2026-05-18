import React, {
    useCallback,
    useEffect,
    useLayoutEffect,
    useMemo,
    useRef,
    useState,
} from "react";
import {
    Virtuoso,
    type Components,
    type VirtuosoHandle,
} from "react-virtuoso";

import { applyScrollHighlight } from "./scrollHighlight";
import {
    VIRTUAL_OVERSCAN_PX,
    VIRTUAL_ROW_GAP_PX,
} from "./virtualizedMessageListLayout";
import {
    findFirstLiveSuffixIndex,
    useMessageListElements,
    type RenderableConversationItem,
    type UseMessageListElementsProps,
} from "./useMessageListElements";

interface VirtuosoMessageListProps extends UseMessageListElementsProps {
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    pendingScrollMessageId: number | null;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    smartScroll: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
}

interface VirtuosoMessageListContext {
    liveItems: RenderableConversationItem[];
}

const MAX_SCROLL_HIGHLIGHT_ATTEMPTS = 60;
const DEFAULT_ITEM_HEIGHT = 160;

function itemMatchesMessageId(
    item: RenderableConversationItem,
    messageId: number,
): boolean {
    return (
        item.messageId === messageId
        || item.messageIds?.includes(messageId)
        || false
    );
}

function recordVirtualRowHeight(key: string, element: HTMLElement | null) {
    if (!element) {
        return;
    }

    const height = element.offsetHeight;
    if (height > 0) {
        window.__AIPP_CHAT_PERF_CAPTURE__?.recordVirtualRowHeight?.(key, height);
    }
}

function useRecordedHeight(
    key: string,
    elementRef: React.RefObject<HTMLDivElement | null>,
) {
    useEffect(() => {
        const element = elementRef.current;
        if (!element) {
            return;
        }

        recordVirtualRowHeight(key, element);

        const observer = new ResizeObserver(() => {
            recordVirtualRowHeight(key, element);
        });
        observer.observe(element);

        return () => {
            observer.disconnect();
        };
    }, [elementRef, key]);
}

interface MeasuredVirtuosoItemProps {
    item: RenderableConversationItem;
    hasGapAfter: boolean;
}

const MeasuredVirtuosoItem = React.memo(
    ({ item, hasGapAfter }: MeasuredVirtuosoItemProps) => {
        const rowRef = useRef<HTMLDivElement | null>(null);
        useRecordedHeight(item.key, rowRef);

        return (
            <div
                ref={rowRef}
                style={{
                    boxSizing: "border-box",
                    paddingBottom: hasGapAfter ? VIRTUAL_ROW_GAP_PX : 0,
                }}
            >
                {item.element}
            </div>
        );
    },
);

MeasuredVirtuosoItem.displayName = "MeasuredVirtuosoItem";

const VirtuosoLiveFooter = React.memo(
    ({ context }: { context: VirtuosoMessageListContext }) => {
        const footerRef = useRef<HTMLDivElement | null>(null);
        const { liveItems } = context;
        const footerKey = useMemo(
            () => `live-tail:${liveItems.map((item) => item.key).join("|")}`,
            [liveItems],
        );

        useRecordedHeight(footerKey, footerRef);

        if (liveItems.length === 0) {
            return null;
        }

        return (
            <div ref={footerRef} className="flex flex-col gap-4">
                {liveItems.map((item) => (
                    <React.Fragment key={item.key}>{item.element}</React.Fragment>
                ))}
            </div>
        );
    },
);

VirtuosoLiveFooter.displayName = "VirtuosoLiveFooter";

const virtuosoComponents: Components<
    RenderableConversationItem,
    VirtuosoMessageListContext
> = {
    Footer: VirtuosoLiveFooter,
};

function scheduleHighlightAttempt({
    container,
    messageId,
    setShiningMessageIds,
    clearPendingScrollMessageId,
}: {
    container: HTMLElement;
    messageId: number;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    clearPendingScrollMessageId: (messageId: number | null) => void;
}) {
    let attempts = 0;
    const tryHighlight = () => {
        const target = container.querySelector(
            `[data-message-id='${messageId}']`,
        ) as HTMLElement | null;
        if (target) {
            applyScrollHighlight({
                target,
                messageId,
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
}

const VirtuosoMessageList: React.FC<VirtuosoMessageListProps> = ({
    scrollContainerRef,
    pendingScrollMessageId,
    clearPendingScrollMessageId,
    setShiningMessageIds,
    onScrollStateChange,
    ...messageListProps
}) => {
    const { renderItems } = useMessageListElements(messageListProps);
    const virtuosoRef = useRef<VirtuosoHandle | null>(null);
    const scrollSyncFrameRef = useRef<number | null>(null);
    const [scrollParent, setScrollParent] = useState<HTMLDivElement | null>(null);
    const [viewportHeight, setViewportHeight] = useState(0);

    const firstLiveIndex = useMemo(
        () => findFirstLiveSuffixIndex(renderItems),
        [renderItems],
    );
    const historyItems = useMemo(
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
    const heightEstimates = useMemo(
        () =>
            historyItems.map((item, index) => {
                const hasGapAfter =
                    index < historyItems.length - 1 || liveItems.length > 0;
                return item.estimatedHeight + (hasGapAfter ? VIRTUAL_ROW_GAP_PX : 0);
            }),
        [historyItems, liveItems.length],
    );
    const defaultItemHeight = useMemo(() => {
        if (heightEstimates.length === 0) {
            return DEFAULT_ITEM_HEIGHT;
        }

        const sample = heightEstimates.slice(0, 12);
        return Math.max(
            1,
            Math.round(
                sample.reduce((sum, height) => sum + height, 0) / sample.length,
            ),
        );
    }, [heightEstimates]);
    const context = useMemo<VirtuosoMessageListContext>(
        () => ({ liveItems }),
        [liveItems],
    );

    useLayoutEffect(() => {
        setScrollParent(scrollContainerRef.current);
    }, [scrollContainerRef]);

    useEffect(() => {
        const container = scrollParent;
        if (!container) {
            return;
        }

        const syncScrollState = () => {
            if (scrollSyncFrameRef.current !== null) {
                return;
            }

            scrollSyncFrameRef.current = requestAnimationFrame(() => {
                scrollSyncFrameRef.current = null;
                setViewportHeight(container.clientHeight);
                onScrollStateChange?.(container);
            });
        };

        syncScrollState();
        container.addEventListener("scroll", syncScrollState, { passive: true });
        const resizeObserver = new ResizeObserver(syncScrollState);
        resizeObserver.observe(container);

        return () => {
            if (scrollSyncFrameRef.current !== null) {
                cancelAnimationFrame(scrollSyncFrameRef.current);
                scrollSyncFrameRef.current = null;
            }
            container.removeEventListener("scroll", syncScrollState);
            resizeObserver.disconnect();
        };
    }, [onScrollStateChange, scrollParent]);
    const overscanPx = useMemo(
        () => Math.max(VIRTUAL_OVERSCAN_PX, viewportHeight * 8),
        [viewportHeight],
    );

    const itemContent = useCallback(
        (index: number, item: RenderableConversationItem) => (
            <MeasuredVirtuosoItem
                item={item}
                hasGapAfter={index < historyItems.length - 1 || liveItems.length > 0}
            />
        ),
        [historyItems.length, liveItems.length],
    );

    useEffect(() => {
        if (pendingScrollMessageId === null) {
            return;
        }

        const container = scrollParent;
        if (!container) {
            return;
        }

        const targetIsLive = liveItems.some((item) =>
            itemMatchesMessageId(item, pendingScrollMessageId),
        );
        if (targetIsLive) {
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
                    top: Math.max(0, container.scrollHeight - container.clientHeight),
                    behavior: "smooth",
                });
            }

            scheduleHighlightAttempt({
                container,
                messageId: pendingScrollMessageId,
                setShiningMessageIds,
                clearPendingScrollMessageId,
            });
            return;
        }

        const targetIndex = historyItems.findIndex((item) =>
            itemMatchesMessageId(item, pendingScrollMessageId),
        );
        if (targetIndex < 0) {
            clearPendingScrollMessageId(null);
            return;
        }

        virtuosoRef.current?.scrollToIndex({
            index: targetIndex,
            align: "center",
            behavior: "smooth",
        });
        scheduleHighlightAttempt({
            container,
            messageId: pendingScrollMessageId,
            setShiningMessageIds,
            clearPendingScrollMessageId,
        });
    }, [
        clearPendingScrollMessageId,
        historyItems,
        liveItems,
        pendingScrollMessageId,
        scrollParent,
        setShiningMessageIds,
    ]);

    if (!scrollParent) {
        return <div style={{ minHeight: 1 }} />;
    }

    if (historyItems.length === 0) {
        return <VirtuosoLiveFooter context={context} />;
    }

    return (
        <Virtuoso
            ref={virtuosoRef}
            customScrollParent={scrollParent}
            data={historyItems}
            computeItemKey={(_index, item) => item.key}
            itemContent={itemContent}
            components={virtuosoComponents}
            context={context}
            defaultItemHeight={defaultItemHeight}
            heightEstimates={heightEstimates}
            increaseViewportBy={{
                top: overscanPx,
                bottom: overscanPx,
            }}
            minOverscanItemCount={{ top: 8, bottom: 8 }}
            alignToBottom
            style={{ width: "100%" }}
        />
    );
};

export default React.memo(VirtuosoMessageList);
