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

import {
    applyScrollHighlight,
    MESSAGE_SCROLL_START_GAP_PX,
    scrollMessageIntoView,
    type MessageScrollAlign,
} from "./scrollHighlight";
import {
    VIRTUAL_OVERSCAN_PX,
    VIRTUAL_ROW_GAP_PX,
} from "./virtualizedMessageListLayout";
import {
    followScrollContainerBottom,
    pinScrollContainerToBottom,
} from "./pinScrollToBottom";
import { playSentBubbleAnimation } from "./sentBubbleAnimation";
import { startSendSlide, type SendSlide } from "./sendSlideAnimation";
import {
    findFirstLiveSuffixIndex,
    useMessageListElements,
    type RenderableConversationItem,
    type UseMessageListElementsProps,
} from "./useMessageListElements";

interface VirtuosoMessageListProps extends UseMessageListElementsProps {
    conversationId: string;
    /**
     * 列表数据的身份。默认按每条消息的 conversation_id 判断数据是否已属于当前会话
     * （切会话时旧数据还没换掉，不能拿它做打开贴底）。群聊等聚合视图的消息分属多个执行会话，
     * 由宿主直接给出：与 conversationId 相同即视为数据已就绪。
     */
    transcriptKey?: string;
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    pendingScrollMessageId: number | null;
    /** 跳转对齐方式，默认居中；轮次导航使用 start。 */
    pendingScrollAlign?: MessageScrollAlign;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    setShiningMessageIds: React.Dispatch<React.SetStateAction<Set<number>>>;
    onScrollStateChange?: (container?: HTMLDivElement | null) => void;
    smartScroll: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
}

interface VirtuosoMessageListContext {
    liveItems: RenderableConversationItem[];
}

const MAX_SCROLL_HIGHLIGHT_ATTEMPTS = 180;
const INITIAL_BOTTOM_PIN_MIN_FRAME_COUNT = 10;
const INITIAL_BOTTOM_PIN_MAX_FRAME_COUNT = 90;
const INITIAL_BOTTOM_PIN_STABLE_FRAME_COUNT = 4;
/** 硬超时：pin 被打断时也不能永久 visibility:hidden；长对话布局常超过 500ms */
const INITIAL_BOTTOM_PIN_FAILSAFE_MS = 2500;
/** 打开后继续跟随晚到布局（代码高亮 / 图片 / Virtuoso 估高校正） */
const INITIAL_BOTTOM_FOLLOW_MAX_DURATION_MS = 15_000;
/** 流式输出期间持续贴底；实际结束由 streamingMessages 清空时的 effect cleanup 负责 */
const STREAM_FOLLOW_MAX_DURATION_MS = 10 * 60_000;

function readContentWidth(element: HTMLElement): number {
    const style = getComputedStyle(element);
    return Math.max(
        0,
        element.clientWidth
            - (parseFloat(style.paddingLeft) || 0)
            - (parseFloat(style.paddingRight) || 0),
    );
}

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

function useRecordedHeight(
    key: string,
    elementRef: React.RefObject<HTMLDivElement | null>,
    estimatedHeight?: number,
) {
    const estimatedHeightRef = useRef(estimatedHeight);
    estimatedHeightRef.current = estimatedHeight;
    useEffect(() => {
        const element = elementRef.current;
        const recordHeight = window.__AIPP_CHAT_PERF_CAPTURE__?.recordVirtualRowHeight;
        if (!element || !recordHeight) {
            return;
        }

        const observer = new ResizeObserver((entries) => {
            const entry = entries[0];
            if (!entry) return;
            const height = entry.borderBoxSize[0]?.blockSize ?? entry.contentRect.height;
            if (height <= 0) return;
            const parts = Array.from(
                element.querySelectorAll<HTMLElement>("[data-message-id][data-message-type]"),
                (part) => ({
                    id: part.dataset.messageId ?? "",
                    type: part.dataset.messageType ?? "",
                    height: Math.round(part.getBoundingClientRect().height),
                }),
            );
            recordHeight(key, height, estimatedHeightRef.current, parts);
        });
        observer.observe(element, { box: "border-box" });

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

function historyRowHasGapAfter(
    item: Pick<RenderableConversationItem, "joinsNext">,
    index: number,
    historyCount: number,
    liveCount: number,
): boolean {
    return !item.joinsNext && (index < historyCount - 1 || liveCount > 0);
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
        useRecordedHeight(
            item.key,
            rowRef,
            item.estimatedHeight + (hasGapAfter ? VIRTUAL_ROW_GAP_PX : 0),
        );

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
            <div ref={footerRef} className="flex flex-col">
                {liveItems.map((item, index) => (
                    <React.Fragment key={item.key}>
                        {item.element}
                        {index < liveItems.length - 1 && !item.joinsNext && (
                            <div aria-hidden className="flex-none" style={{ height: VIRTUAL_ROW_GAP_PX }} />
                        )}
                    </React.Fragment>
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
    align,
    setShiningMessageIds,
    clearPendingScrollMessageId,
}: {
    container: HTMLElement;
    messageId: number;
    align: MessageScrollAlign;
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
                align,
                container,
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
    transcriptKey,
    scrollContainerRef,
    pendingScrollMessageId,
    pendingScrollAlign = "center",
    clearPendingScrollMessageId,
    setShiningMessageIds,
    onScrollStateChange,
    ...messageListProps
}) => {
    const [listWidth, setListWidth] = useState<number | null>(null);
    const { renderItems } = useMessageListElements({
        ...messageListProps,
        listWidthPx: listWidth || undefined,
    });
    // Keep bounded rich transcripts mounted: viewport crossings must not
    // repeatedly recreate code blocks and change the reader's scroll anchor.
    const retainHistory = messageListProps.allDisplayMessages.length <= 80
        && messageListProps.allDisplayMessages.reduce(
            (total, message) => total + (message.content?.length ?? 0), 0,
        ) <= 500_000;
    const virtuosoRef = useRef<VirtuosoHandle | null>(null);
    const listWrapperRef = useRef<HTMLDivElement | null>(null);
    const scrollSyncFrameRef = useRef<number | null>(null);
    const initialBottomConversationRef = useRef<string | null>(null);
    const initialBottomPinConversationRef = useRef<string | null>(null);
    /** 打开后用户已用滚轮或滚动条上翻；效果重跑时不再钉底或跟滚。 */
    const userReleasedInitialScrollRef = useRef<string | null>(null);
    const lastPinnedUserMessageIdRef = useRef<number | null>(null);
    /** 当前这次发送：视觉滑动 + 用户是否已接手；effect 重跑时沿用 */
    const sendScrollStateRef = useRef<{
        messageId: number;
        releasedByUser: boolean;
        slide: SendSlide | null;
    } | null>(null);
    /** live-only 分支已把本会话贴底展示过；之后长出 history 属于「发送」，不是重新打开 */
    const liveOnlyShownConversationRef = useRef<string | null>(null);
    /** 本会话已带消息渲染过（在 commit 之后写入），用来区分「打开会话」和「发送」 */
    const renderedConversationRef = useRef<string | null>(null);
    /** 正在进行的发送钉底 + 动画；不随 effect cleanup 停止 */
    const sendScrollStopRef = useRef<(() => void) | null>(null);
    const onScrollStateChangeRef = useRef(onScrollStateChange);
    onScrollStateChangeRef.current = onScrollStateChange;
    const stopSendScroll = useCallback(() => {
        const stop = sendScrollStopRef.current;
        sendScrollStopRef.current = null;
        stop?.();
    }, []);
    const isSendScrollAnimating = useCallback(
        () => sendScrollStateRef.current?.slide?.isActive() ?? false,
        [],
    );
    const tailUserMessageIdRef = useRef<number | null>(null);
    const streamingActiveRef = useRef(false);
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
            historyItems.map((item, index) =>
                item.estimatedHeight
                + (historyRowHasGapAfter(item, index, historyItems.length, liveItems.length)
                    ? VIRTUAL_ROW_GAP_PX
                    : 0),
            ),
        [historyItems, liveItems.length],
    );
    const context = useMemo<VirtuosoMessageListContext>(
        () => ({ liveItems }),
        [liveItems],
    );
    const tailUserMessageId = useMemo(() => {
        const lastMessage = messageListProps.allDisplayMessages.at(-1);
        if (!lastMessage || lastMessage.message_type !== "user") {
            return null;
        }

        return lastMessage.id;
    }, [messageListProps.allDisplayMessages]);
    tailUserMessageIdRef.current = tailUserMessageId;
    const liveItemKeys = useMemo(
        () => liveItems.map((item) => item.key).join("|"),
        [liveItems],
    );
    const streamingMessageCount = messageListProps.streamingMessages.size;
    streamingActiveRef.current = streamingMessageCount > 0;
    const hasCurrentConversationMessages = useMemo(() => {
        if (!conversationId || messageListProps.allDisplayMessages.length === 0) {
            return false;
        }
        if (transcriptKey !== undefined) {
            return transcriptKey === conversationId;
        }

        return messageListProps.allDisplayMessages.every(
            (message) => String(message.conversation_id) === conversationId,
        );
    }, [conversationId, messageListProps.allDisplayMessages, transcriptKey]);
    // 按数据实际所属会话重建 Virtuoso，保证新会话从空 sizeTree 开始并采纳本会话的 heightEstimates。
    const dataConversationKey = transcriptKey ?? String(
        messageListProps.allDisplayMessages[0]?.conversation_id ?? conversationId,
    );

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
                const width = readContentWidth(el);
                setListWidth((prev) => (prev === width ? prev : width));
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

    useLayoutEffect(() => {
        stopSendScroll();
        renderedConversationRef.current = null;
        lastPinnedUserMessageIdRef.current = null;
        sendScrollStateRef.current = null;
        liveOnlyShownConversationRef.current = null;
        initialBottomConversationRef.current = null;
        initialBottomPinConversationRef.current = null;
        userReleasedInitialScrollRef.current = null;
        setInitialBottomVisibleConversationId(null);
    }, [conversationId]);

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
        const resizeObserver = new ResizeObserver(() => {
            const width = readContentWidth(container);
            setListWidth((prev) => (prev === width ? prev : width));
            syncScrollState();
        });
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
        if (userReleasedInitialScrollRef.current === conversationId) {
            return;
        }
        if (pendingScrollMessageId !== null && hasCurrentConversationMessages) {
            initialBottomConversationRef.current = conversationId;
            setInitialBottomVisibleConversationId((currentConversationId) =>
                currentConversationId === conversationId
                    ? currentConversationId
                    : conversationId,
            );
            return;
        }

        const markInitialScrollReleased = () => {
            userReleasedInitialScrollRef.current = conversationId;
        };
        const markInitialScrollResumed = () => {
            if (userReleasedInitialScrollRef.current === conversationId) {
                userReleasedInitialScrollRef.current = null;
            }
        };
        const startFollow = (target: HTMLDivElement) =>
            followScrollContainerBottom({
                container: target,
                contentElements: [listWrapperRef.current],
                onScrollStateChange,
                shouldContinue: () =>
                    initialBottomConversationRef.current === conversationId,
                maxDurationMs: INITIAL_BOTTOM_FOLLOW_MAX_DURATION_MS,
                onUserPause: markInitialScrollReleased,
                onUserResume: markInitialScrollResumed,
                suspendWhile: isSendScrollAnimating,
            });

        // live-only 无 Virtuoso 贴底闪动：不 pin、不 hide，一次滚动贴底后继续跟随晚到增高。
        // 不写入 initialBottomConversationRef：若随后长出 history，仍走完整 pin 路径。
        if (historyItems.length === 0) {
            if (
                !container
                || !hasCurrentConversationMessages
            ) {
                return;
            }

            setInitialBottomVisibleConversationId(conversationId);
            // 每个会话只在首次展示时瞬间贴底；之后的重跑（含发送动画进行中）不能再 snap
            if (
                liveOnlyShownConversationRef.current !== conversationId
                && !container.closest('[data-aipp-mobile-shell] [hidden]')
            ) {
                container.scrollTop = Math.max(
                    0,
                    container.scrollHeight - container.clientHeight,
                );
                onScrollStateChange?.(container);
                liveOnlyShownConversationRef.current = conversationId;
            }
            // live-only follow：cleanup 时 stop；shouldContinue 不依赖 pin-done ref
            const stopFollow = followScrollContainerBottom({
                container,
                contentElements: [listWrapperRef.current],
                onScrollStateChange,
                shouldContinue: () => true,
                maxDurationMs: INITIAL_BOTTOM_FOLLOW_MAX_DURATION_MS,
                onUserPause: markInitialScrollReleased,
                onUserResume: markInitialScrollResumed,
                suspendWhile: isSendScrollAnimating,
            });
            return () => {
                stopFollow();
            };
        }

        if (
            !container
            || !hasCurrentConversationMessages
            || initialBottomPinConversationRef.current === conversationId
        ) {
            return;
        }

        // 已以 live-only 贴底展示过：发送后首轮转入 history，交给发送滑动动画，
        // 不能再走一次「隐藏 + 瞬间钉底」的打开流程。
        if (
            liveOnlyShownConversationRef.current === conversationId
            && initialBottomConversationRef.current !== conversationId
        ) {
            initialBottomConversationRef.current = conversationId;
        }

        // 初始 pin 已完成：effect 因依赖变化 cleanup 后必须重新挂上 follow，
        // 否则晚到的布局增高会把视口留在半截（长对话打开常见）。
        if (initialBottomConversationRef.current === conversationId) {
            const stopFollow = startFollow(container);
            return () => {
                stopFollow();
            };
        }

        initialBottomPinConversationRef.current = conversationId;

        let stopFollow: (() => void) | null = null;
        const stopPin = pinScrollContainerToBottom({
            container,
            onScrollStateChange,
            shouldContinue: () =>
                initialBottomPinConversationRef.current === conversationId,
            onComplete: () => {
                initialBottomConversationRef.current = conversationId;
                initialBottomPinConversationRef.current = null;
                if (tailUserMessageIdRef.current !== null) {
                    lastPinnedUserMessageIdRef.current = tailUserMessageIdRef.current;
                }
                setInitialBottomVisibleConversationId(conversationId);
                stopFollow = startFollow(container);
            },
            onUserRelease: () => {
                userReleasedInitialScrollRef.current = conversationId;
                initialBottomConversationRef.current = conversationId;
                initialBottomPinConversationRef.current = null;
                if (tailUserMessageIdRef.current !== null) {
                    lastPinnedUserMessageIdRef.current = tailUserMessageIdRef.current;
                }
                setInitialBottomVisibleConversationId(conversationId);
            },
            minFrameCount: INITIAL_BOTTOM_PIN_MIN_FRAME_COUNT,
            stableFrameCount: INITIAL_BOTTOM_PIN_STABLE_FRAME_COUNT,
            maxFrameCount: INITIAL_BOTTOM_PIN_MAX_FRAME_COUNT,
            observeMode: "full",
            failsafeTimeoutMs: INITIAL_BOTTOM_PIN_FAILSAFE_MS,
        });

        return () => {
            if (initialBottomPinConversationRef.current === conversationId) {
                initialBottomPinConversationRef.current = null;
            }
            stopPin();
            stopFollow?.();
        };
    }, [
        conversationId,
        hasCurrentConversationMessages,
        // 仅用是否存在 history 区分 live-only / virtuoso；避免 live 长度抖动反复 cancel pin
        historyItems.length > 0,
        isSendScrollAnimating,
        onScrollStateChange,
        pendingScrollMessageId,
        effectiveScrollParent,
    ]);

    // 发消息后：缓动滑到底部（user 气泡贴近顶部），再用 ResizeObserver 钉底等布局稳定。
    //
    // 钉底的生命周期刻意不挂在 effect cleanup 上：发送后后端很快追加 assistant 消息，
    // tailUserMessageId 立刻变回 null、liveItemKeys 也在变，effect 会反复重跑。
    // 若随 cleanup 停止，动画会停在半路。这里只在用户接手 / 切会话 / 跳转 / 卸载时停止。
    useLayoutEffect(() => {
        const container = effectiveScrollParent;
        if (!container || !hasCurrentConversationMessages || tailUserMessageId === null) {
            return;
        }

        if (pendingScrollMessageId !== null) {
            stopSendScroll();
            lastPinnedUserMessageIdRef.current = tailUserMessageId;
            return;
        }

        if (initialBottomPinConversationRef.current === conversationId) {
            return;
        }

        if (lastPinnedUserMessageIdRef.current === tailUserMessageId) {
            return;
        }

        // 本会话首次带消息渲染 = 打开会话，不是发送：交给初始贴底流程，不播动画
        if (renderedConversationRef.current !== conversationId) {
            lastPinnedUserMessageIdRef.current = tailUserMessageId;
            return;
        }

        if (
            sendScrollStateRef.current?.messageId === tailUserMessageId
            && sendScrollStopRef.current
        ) {
            return;
        }

        stopSendScroll();
        const pinningMessageId = tailUserMessageId;
        const sendState = {
            messageId: pinningMessageId,
            releasedByUser: false,
            slide: null as SendSlide | null,
        };
        sendScrollStateRef.current = sendState;

        // 每次钉底跳动都在绘制前同步回调：首次创建反向 transform，之后叠加补偿
        const handleScrollDelta = (delta: number) => {
            if (sendState.releasedByUser) {
                return;
            }
            if (sendState.slide) {
                sendState.slide.addOffset(delta);
                return;
            }
            const wrapper = listWrapperRef.current;
            const content = wrapper?.firstElementChild;
            if (!wrapper || !(content instanceof HTMLElement)) {
                return;
            }
            sendState.slide = startSendSlide(wrapper, content, delta);
        };

        const stopBubble = playSentBubbleAnimation(container, pinningMessageId);
        let stopPin: (() => void) | null = null;
        const finishPin = () => {
            lastPinnedUserMessageIdRef.current = pinningMessageId;
            if (sendScrollStopRef.current === stop) {
                sendScrollStopRef.current = null;
            }
        };
        const stop = () => {
            stopPin?.();
            stopBubble();
            sendState.slide?.cancel();
        };
        sendScrollStopRef.current = stop;

        stopPin = pinScrollContainerToBottom({
            container,
            onScrollStateChange: (target) => onScrollStateChangeRef.current?.(target),
            shouldContinue: () => true,
            onComplete: finishPin,
            onUserRelease: () => {
                sendState.releasedByUser = true;
                finishPin();
            },
            observeMode: "tail",
            onScrollDelta: handleScrollDelta,
            holdWhile: () => sendState.slide?.isActive() ?? false,
        });
    }, [
        conversationId,
        hasCurrentConversationMessages,
        liveItemKeys,
        pendingScrollMessageId,
        effectiveScrollParent,
        stopSendScroll,
        tailUserMessageId,
    ]);

    // 记录「本会话已经带消息渲染过」：之后出现新的 tail user 消息才算发送
    useEffect(() => {
        if (
            hasCurrentConversationMessages
            || messageListProps.allDisplayMessages.length === 0
        ) {
            renderedConversationRef.current = conversationId;
        }
    }, [
        conversationId,
        hasCurrentConversationMessages,
        messageListProps.allDisplayMessages.length,
    ]);

    // 换了滚动容器或卸载时停止发送滑动
    useEffect(() => stopSendScroll, [effectiveScrollParent, stopSendScroll]);

    // 流式回复持续长高：follow 贴底；用户上滑暂停、滚回底部恢复（见 followScrollContainerBottom）。
    useLayoutEffect(() => {
        const container = effectiveScrollParent;
        if (
            !container
            || streamingMessageCount === 0
            || !hasCurrentConversationMessages
            || pendingScrollMessageId !== null
        ) {
            return;
        }

        // 打开会话的初始 pin 进行中时不要抢 follow；pin onComplete 会接上打开 follow
        if (initialBottomPinConversationRef.current === conversationId) {
            return;
        }

        return followScrollContainerBottom({
            container,
            contentElements: [listWrapperRef.current],
            onScrollStateChange,
            shouldContinue: () => streamingActiveRef.current,
            maxDurationMs: STREAM_FOLLOW_MAX_DURATION_MS,
            suspendWhile: isSendScrollAnimating,
            // 发送滑动途中用户已上滑：流式开始时不要再把人拽回底部
            startPaused: sendScrollStateRef.current?.releasedByUser === true,
        });
    }, [
        conversationId,
        hasCurrentConversationMessages,
        isSendScrollAnimating,
        onScrollStateChange,
        pendingScrollMessageId,
        effectiveScrollParent,
        streamingMessageCount,
    ]);

    const overscanPx = useMemo(
        () => Math.max(VIRTUAL_OVERSCAN_PX, viewportHeight * 1.5),
        [viewportHeight],
    );
    const initialBottomOffsetPx = useMemo(() => {
        if (liveItems.length === 0) {
            return 0;
        }

        return -liveItems.reduce((sum, item, index) => {
            const hasGapAfter = index < liveItems.length - 1 && !item.joinsNext;
            return sum + item.estimatedHeight + (hasGapAfter ? VIRTUAL_ROW_GAP_PX : 0);
        }, 0);
    }, [liveItems]);
    // history 在初始贴底时暂时隐藏；无滚动容器时不会启动 pin，不能隐藏消息。
    // live-only（短会话 / 总管家常见）没有 Virtuoso 定位，绝不能藏气泡。
    const shouldHideInitialBottomPositioning =
        historyItems.length > 0
        && !!effectiveScrollParent
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
                hasGapAfter={historyRowHasGapAfter(
                    item,
                    index,
                    historyItems.length,
                    liveItems.length,
                )}
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
        if (targetIsLive || retainHistory) {
            const existingTarget = container.querySelector(
                `[data-message-id='${pendingScrollMessageId}']`,
            ) as HTMLElement | null;
            if (existingTarget) {
                scrollMessageIntoView(existingTarget, pendingScrollAlign, container);
            } else {
                container.scrollTo({
                    top: Math.max(0, container.scrollHeight - container.clientHeight),
                    behavior: "smooth",
                });
            }

            scheduleHighlightAttempt({
                container,
                messageId: pendingScrollMessageId,
                align: pendingScrollAlign,
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
            align: pendingScrollAlign,
            // start 对齐时预留顶部空隙；渲染后高亮阶段会按实测位置再精确校正一次
            offset: pendingScrollAlign === "start" ? -MESSAGE_SCROLL_START_GAP_PX : 0,
            behavior: "auto",
        });
        scheduleHighlightAttempt({
            container,
            messageId: pendingScrollMessageId,
            align: pendingScrollAlign,
            setShiningMessageIds,
            clearPendingScrollMessageId,
        });
    }, [
        clearPendingScrollMessageId,
        historyItems,
        liveItems,
        pendingScrollMessageId,
        pendingScrollAlign,
        effectiveScrollParent,
        setShiningMessageIds,
        retainHistory,
    ]);

    // live-only：不依赖 scrollParent，直接渲染（总管家短会话常见路径）
    // 与 retainHistory 分支保持同一 DOM 结构（wrapper > footer）：首轮转入 history 时
    // 不会整棵重挂载所有消息（那一下的主线程卡顿正好落在发送动画上），发送滑动也需要 wrapper。
    if (historyItems.length === 0) {
        return (
            <div ref={listWrapperRef}>
                <VirtuosoLiveFooter context={{ liveItems }} />
            </div>
        );
    }

    if (retainHistory) {
        return (
            <div
                ref={listWrapperRef}
                data-aipp-initial-bottom-positioning={shouldHideInitialBottomPositioning ? "true" : undefined}
                style={initialBottomPositioningStyle}
            >
                <VirtuosoLiveFooter context={{ liveItems: renderItems }} />
            </div>
        );
    }

    // history 路径需要 customScrollParent；ref 尚未就绪时：
    // - 短列表可先直出，避免空白
    // - 长列表只露出 live 尾部，避免普通 ChatUI 长会话一次性挂载全部 history
    if (!effectiveScrollParent) {
        const fallbackItems =
            renderItems.length <= 40 ? renderItems : liveItems;
        return <VirtuosoLiveFooter context={{ liveItems: fallbackItems }} />;
    }

    // Virtuoso 只在 sizeTree 为空时采纳第一份非空 heightEstimates，之后的更新全部忽略；
    // 必须等拿到真实列表宽度再挂载，否则按默认宽度估算的高度会被永久固化。
    if (listWidth === null) {
        return <div ref={listWrapperRef} style={initialBottomPositioningStyle} />;
    }

    return (
        <div
            ref={listWrapperRef}
            data-aipp-initial-bottom-positioning={
                shouldHideInitialBottomPositioning ? "true" : undefined
            }
            style={initialBottomPositioningStyle}
        >
            <Virtuoso
                key={dataConversationKey}
                ref={virtuosoRef}
                customScrollParent={effectiveScrollParent}
                data={historyItems}
                computeItemKey={(_index, item) => item.key}
                itemContent={itemContent}
                components={virtuosoComponents}
                context={context}
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
                minOverscanItemCount={{ top: 4, bottom: 4 }}
                alignToBottom
                style={{ width: "100%" }}
            />
        </div>
    );
};

export default React.memo(VirtuosoMessageList);
