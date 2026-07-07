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
}

const MAX_SCROLL_HIGHLIGHT_ATTEMPTS = 180;
const DEFAULT_ITEM_HEIGHT = 160;
const INITIAL_BOTTOM_PIN_MIN_FRAME_COUNT = 10;
const INITIAL_BOTTOM_PIN_MAX_FRAME_COUNT = 30;
const INITIAL_BOTTOM_PIN_STABLE_FRAME_COUNT = 4;
/** 硬超时：pin 被打断时也不能永久 visibility:hidden */
const INITIAL_BOTTOM_PIN_FAILSAFE_MS = 500;

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
        const { liveItems } = context;
        const footerKey = useMemo(
            () => `live-tail:${liveItems.map((item) => item.key).join("|")}`,
            [liveItems],
        );

        useRecordedHeight(footerKey, footerRef);

        if (liveItems.length === 0) {
            return null;
        }

        // last-reply 的 min-height 直接读 live-only CSS 变量，无需再在 footer 上覆盖
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

    // scrollParent 解析不能只依赖 ref 对象 identity：Butler 等场景下首帧
    // ref.current 偶发仍为 null，若不再重试会永久停在 minHeight:1 占位，气泡全无。
    useLayoutEffect(() => {
        const sync = () => {
            const el = scrollContainerRef.current;
            setScrollParent((prev) => (prev === el ? prev : el));
            if (el) {
                setViewportHeight((prev) =>
                    prev === el.clientHeight ? prev : el.clientHeight,
                );
            }
            return el;
        };

        if (sync()) {
            return;
        }

        const frameId = requestAnimationFrame(() => {
            sync();
        });
        return () => {
            cancelAnimationFrame(frameId);
        };
    }, [
        conversationId,
        messageListProps.allDisplayMessages.length,
        scrollContainerRef,
    ]);

    const effectiveScrollParent = scrollParent ?? scrollContainerRef.current;

    useEffect(() => {
        const container = effectiveScrollParent;
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
    }, [onScrollStateChange, effectiveScrollParent]);

    useLayoutEffect(() => {
        const container = effectiveScrollParent;
        if (pendingScrollMessageId !== null && hasCurrentConversationMessages) {
            initialBottomConversationRef.current = conversationId;
            setInitialBottomVisibleConversationId((currentConversationId) =>
                currentConversationId === conversationId
                    ? currentConversationId
                    : conversationId,
            );
            return;
        }

        // live-only 无 Virtuoso 贴底闪动：不 pin、不 hide，仅做一次滚动贴底
        // 同时标记 visible，避免之后长出 history 时因 ref 已写入却未 unhide 而永久隐藏
        if (historyItems.length === 0) {
            if (
                container
                && hasCurrentConversationMessages
                && initialBottomConversationRef.current !== conversationId
            ) {
                initialBottomConversationRef.current = conversationId;
                setInitialBottomVisibleConversationId(conversationId);
                container.scrollTop = Math.max(
                    0,
                    container.scrollHeight - container.clientHeight,
                );
                onScrollStateChange?.(container);
            }
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
        let failsafeTimerId: number | null = null;
        const finishPinAndShow = () => {
            initialBottomConversationRef.current = conversationId;
            initialBottomPinConversationRef.current = null;
            initialBottomPinFrameRef.current = null;
            container.removeEventListener("scroll", handlePinnedScroll);
            contentResizeObserver.disconnect();
            mutationObserver.disconnect();
            if (failsafeTimerId !== null) {
                window.clearTimeout(failsafeTimerId);
                failsafeTimerId = null;
            }
            setInitialBottomVisibleConversationId(conversationId);
        };
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

            const canShowAfterStableLayout =
                elapsedFrames >= INITIAL_BOTTOM_PIN_MIN_FRAME_COUNT
                && stableFrames >= INITIAL_BOTTOM_PIN_STABLE_FRAME_COUNT;
            const reachedMaxWait =
                elapsedFrames >= INITIAL_BOTTOM_PIN_MAX_FRAME_COUNT;
            if (canShowAfterStableLayout || reachedMaxWait) {
                finishPinAndShow();
                return;
            }

            initialBottomPinFrameRef.current = requestAnimationFrame(scrollToBottom);
        };
        // 硬超时：即便 rAF 被反复打断，也不能永久 visibility:hidden
        failsafeTimerId = window.setTimeout(() => {
            if (initialBottomConversationRef.current === conversationId) {
                return;
            }
            pinToBottom();
            finishPinAndShow();
        }, INITIAL_BOTTOM_PIN_FAILSAFE_MS);

        container.addEventListener("scroll", handlePinnedScroll, { passive: true });
        observeContentElements();
        mutationObserver.observe(container, {
            childList: true,
            subtree: true,
        });
        scrollToBottom();

        return () => {
            if (failsafeTimerId !== null) {
                window.clearTimeout(failsafeTimerId);
            }
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
        // 仅用是否存在 history 区分 live-only / virtuoso；避免 live 长度抖动反复 cancel pin
        historyItems.length > 0,
        onScrollStateChange,
        pendingScrollMessageId,
        effectiveScrollParent,
    ]);

    const overscanPx = useMemo(
        () => Math.max(VIRTUAL_OVERSCAN_PX, viewportHeight * 3),
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
    // 仅 Virtuoso history 路径需要 hide 掩盖 initialTopMostItemIndex 闪动；
    // live-only（短会话 / 总管家常见）没有 Virtuoso 定位，绝不能藏气泡。
    const shouldHideInitialBottomPositioning =
        historyItems.length > 0
        && pendingScrollMessageId === null
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

        const container = effectiveScrollParent;
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
            behavior: "auto",
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
        effectiveScrollParent,
        setShiningMessageIds,
    ]);

    // live-only：不依赖 scrollParent，直接渲染（总管家短会话常见路径）
    if (historyItems.length === 0) {
        return <VirtuosoLiveFooter context={{ liveItems }} />;
    }

    // history 路径需要 customScrollParent；ref 尚未就绪时：
    // - 短列表可先直出，避免空白
    // - 长列表只露出 live 尾部，避免普通 ChatUI 长会话一次性挂载全部 history
    if (!effectiveScrollParent) {
        const fallbackItems =
            renderItems.length <= 40 ? renderItems : liveItems;
        return <VirtuosoLiveFooter context={{ liveItems: fallbackItems }} />;
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
                customScrollParent={effectiveScrollParent}
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
