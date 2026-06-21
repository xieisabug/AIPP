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
    CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR,
    CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR,
} from "./layoutConstants";
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
    conversationId: string;
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    pendingScrollMessageId: number | null;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    smartScroll: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
}

interface VirtuosoMessageListContext {
    liveItems: RenderableConversationItem[];
    useLiveOnlyViewportHeight?: boolean;
}

const MAX_SCROLL_HIGHLIGHT_ATTEMPTS = 60;
const DEFAULT_ITEM_HEIGHT = 160;
const INITIAL_BOTTOM_PIN_MIN_FRAME_COUNT = 10;
const INITIAL_BOTTOM_PIN_MAX_FRAME_COUNT = 30;
const INITIAL_BOTTOM_PIN_STABLE_FRAME_COUNT = 4;
const LIVE_ONLY_FOOTER_STYLE = {
    [CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR]: `var(${CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR}, var(${CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR}, 0px))`,
} as React.CSSProperties;

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
    minHeight?: number;
}

export function getVirtuosoRowMinHeight(
    index: number,
    item: Pick<RenderableConversationItem, "estimatedHeight">,
): number | undefined {
    return index === 0 ? item.estimatedHeight : undefined;
}

const MeasuredVirtuosoItem = React.memo(
    ({ item, hasGapAfter, minHeight }: MeasuredVirtuosoItemProps) => {
        const rowRef = useRef<HTMLDivElement | null>(null);
        useRecordedHeight(item.key, rowRef);

        return (
            <div
                ref={rowRef}
                style={{
                    boxSizing: "border-box",
                    minHeight,
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
        const { liveItems, useLiveOnlyViewportHeight = false } = context;
        const footerKey = useMemo(
            () => `live-tail:${liveItems.map((item) => item.key).join("|")}`,
            [liveItems],
        );

        useRecordedHeight(footerKey, footerRef);

        if (liveItems.length === 0) {
            return null;
        }

        return (
            <div
                ref={footerRef}
                className="flex flex-col gap-4"
                style={
                    useLiveOnlyViewportHeight
                        ? LIVE_ONLY_FOOTER_STYLE
                        : undefined
                }
            >
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
    conversationId,
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
    const initialBottomConversationRef = useRef<string | null>(null);
    const initialBottomPinConversationRef = useRef<string | null>(null);
    const initialBottomPinFrameRef = useRef<number | null>(null);
    const [scrollParent, setScrollParent] = useState<HTMLDivElement | null>(null);
    const [viewportHeight, setViewportHeight] = useState(0);
    const [
        initialBottomVisibleConversationId,
        setInitialBottomVisibleConversationId,
    ] = useState<string | null>(null);

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
    const hasCurrentConversationMessages = useMemo(() => {
        if (!conversationId || messageListProps.allDisplayMessages.length === 0) {
            return false;
        }

        return messageListProps.allDisplayMessages.every(
            (message) => String(message.conversation_id) === conversationId,
        );
    }, [conversationId, messageListProps.allDisplayMessages]);

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

    useLayoutEffect(() => {
        const container = scrollParent;
        if (pendingScrollMessageId !== null && hasCurrentConversationMessages) {
            initialBottomConversationRef.current = conversationId;
            setInitialBottomVisibleConversationId((currentConversationId) =>
                currentConversationId === conversationId
                    ? currentConversationId
                    : conversationId,
            );
            return;
        }

        if (
            !container
            || !hasCurrentConversationMessages
            || initialBottomConversationRef.current === conversationId
            || initialBottomPinConversationRef.current === conversationId
        ) {
            return;
        }

        initialBottomPinConversationRef.current = conversationId;
        let elapsedFrames = 0;
        let stableFrames = 0;
        let lastMaxScrollTop: number | null = null;
        let isPinningScroll = false;
        const pinToBottom = () => {
            isPinningScroll = true;
            container.scrollTop = Math.max(
                0,
                container.scrollHeight - container.clientHeight,
            );
            isPinningScroll = false;
            onScrollStateChange?.(container);
        };
        const handlePinnedScroll = () => {
            if (isPinningScroll) {
                return;
            }

            pinToBottom();
        };
        const observedElements = new Set<Element>();
        const contentResizeObserver = new ResizeObserver(() => {
            pinToBottom();
        });
        const observeContentElements = () => {
            const elements = [
                ...Array.from(container.children),
                ...Array.from(container.querySelectorAll("*")),
            ];
            elements.forEach((element) => {
                if (observedElements.has(element)) {
                    return;
                }

                observedElements.add(element);
                contentResizeObserver.observe(element);
            });
        };
        const mutationObserver = new MutationObserver(() => {
            observeContentElements();
            pinToBottom();
        });
        const scrollToBottom = () => {
            if (initialBottomPinConversationRef.current !== conversationId) {
                return;
            }

            pinToBottom();
            elapsedFrames += 1;

            const maxScrollTop = Math.max(
                0,
                container.scrollHeight - container.clientHeight,
            );
            if (
                lastMaxScrollTop !== null
                && maxScrollTop > 0
                && Math.abs(maxScrollTop - lastMaxScrollTop) <= 1
            ) {
                stableFrames += 1;
            } else {
                stableFrames = 0;
            }
            lastMaxScrollTop = maxScrollTop;

            const canShowAfterStableLayout =
                elapsedFrames >= INITIAL_BOTTOM_PIN_MIN_FRAME_COUNT
                && stableFrames >= INITIAL_BOTTOM_PIN_STABLE_FRAME_COUNT;
            const reachedMaxWait =
                elapsedFrames >= INITIAL_BOTTOM_PIN_MAX_FRAME_COUNT;
            if (canShowAfterStableLayout || reachedMaxWait) {
                initialBottomConversationRef.current = conversationId;
                initialBottomPinConversationRef.current = null;
                initialBottomPinFrameRef.current = null;
                container.removeEventListener("scroll", handlePinnedScroll);
                contentResizeObserver.disconnect();
                mutationObserver.disconnect();
                setInitialBottomVisibleConversationId(conversationId);
                return;
            }

            initialBottomPinFrameRef.current = requestAnimationFrame(scrollToBottom);
        };

        container.addEventListener("scroll", handlePinnedScroll, { passive: true });
        observeContentElements();
        mutationObserver.observe(container, {
            childList: true,
            subtree: true,
        });
        scrollToBottom();

        return () => {
            if (initialBottomPinConversationRef.current === conversationId) {
                initialBottomPinConversationRef.current = null;
            }
            container.removeEventListener("scroll", handlePinnedScroll);
            contentResizeObserver.disconnect();
            mutationObserver.disconnect();
            if (initialBottomPinFrameRef.current !== null) {
                cancelAnimationFrame(initialBottomPinFrameRef.current);
                initialBottomPinFrameRef.current = null;
            }
        };
    }, [
        conversationId,
        hasCurrentConversationMessages,
        historyItems.length,
        liveItems.length,
        onScrollStateChange,
        pendingScrollMessageId,
        scrollParent,
    ]);

    const overscanPx = useMemo(
        () => Math.max(VIRTUAL_OVERSCAN_PX, viewportHeight * 8),
        [viewportHeight],
    );
    const initialBottomOffsetPx = useMemo(() => {
        if (liveItems.length === 0) {
            return 0;
        }

        return -liveItems.reduce((sum, item, index) => {
            const hasGapAfter = index < liveItems.length - 1;
            return sum + item.estimatedHeight + (hasGapAfter ? VIRTUAL_ROW_GAP_PX : 0);
        }, 0);
    }, [liveItems]);
    const shouldHideInitialBottomPositioning =
        pendingScrollMessageId === null
        && hasCurrentConversationMessages
        && initialBottomVisibleConversationId !== conversationId;
    const initialBottomPositioningStyle = shouldHideInitialBottomPositioning
        ? {
            visibility: "hidden",
        } satisfies React.CSSProperties
        : undefined;

    const itemContent = useCallback(
        (index: number, item: RenderableConversationItem) => (
            <MeasuredVirtuosoItem
                item={item}
                hasGapAfter={index < historyItems.length - 1 || liveItems.length > 0}
                minHeight={getVirtuosoRowMinHeight(index, item)}
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
        return (
            <div
                data-aipp-initial-bottom-positioning={
                    shouldHideInitialBottomPositioning ? "true" : undefined
                }
                style={initialBottomPositioningStyle}
            >
                <VirtuosoLiveFooter
                    context={{
                        ...context,
                        useLiveOnlyViewportHeight: true,
                    }}
                />
            </div>
        );
    }

    return (
        <div
            data-aipp-initial-bottom-positioning={
                shouldHideInitialBottomPositioning ? "true" : undefined
            }
            style={initialBottomPositioningStyle}
        >
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
                initialTopMostItemIndex={{
                    index: "LAST",
                    align: "end",
                    offset: initialBottomOffsetPx,
                }}
                increaseViewportBy={{
                    top: overscanPx,
                    bottom: overscanPx,
                }}
                minOverscanItemCount={{ top: 8, bottom: 8 }}
                alignToBottom
                style={{ width: "100%" }}
            />
        </div>
    );
};

export default React.memo(VirtuosoMessageList);
