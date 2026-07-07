import { useRef, useCallback, useEffect } from "react";
import type { TouchEvent, WheelEvent } from "react";
import {
    CHAT_SCROLL_HISTORY_TAIL_VIEWPORT_HEIGHT_CSS_VAR,
    CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR,
    CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR,
    computeChatViewportHeights,
} from "@/components/conversation/layoutConstants";

const SMOOTH_SCROLL_LOCK_MS = 350;
const AUTO_SCROLL_LOCK_MS = 100;
const USER_SCROLL_SUPPRESSION_MS = 250;

function isChatScrollPerfProbeActive(): boolean {
    return (
        typeof window !== "undefined"
        && (window as Window & { __AIPP_CHAT_SCROLL_PERF_ACTIVE__?: boolean })
            .__AIPP_CHAT_SCROLL_PERF_ACTIVE__ === true
    );
}

export interface UseScrollManagementReturn {
    messagesEndRef: React.RefObject<HTMLDivElement | null>;
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    handleScroll: () => void;
    handleUserScrollIntent: (
        event?: WheelEvent<HTMLDivElement> | TouchEvent<HTMLDivElement>,
    ) => void;
    syncScrollState: (container?: HTMLDivElement | null) => void;
    // Allow overriding behavior for specific scenarios (e.g., instant on open)
    smartScroll: (forceScroll?: boolean, behaviorOverride?: ScrollBehavior) => void;
    scrollToUserMessage: () => void;
    // 虚拟化列表发送消息后稳定滚到底部（多帧钉底，容忍布局延迟增长）
    scrollToBottomStable: () => void;
}

interface UseScrollManagementOptions {
    disableTailObservation?: boolean;
}

export function useScrollManagement(
    options: UseScrollManagementOptions = {},
): UseScrollManagementReturn {
    const { disableTailObservation = false } = options;
    // 滚动相关状态和逻辑
    const messagesEndRef = useRef<HTMLDivElement | null>(null);
    const scrollContainerRef = useRef<HTMLDivElement | null>(null);
    const isUserScrolledUpRef = useRef(false); // 使用 Ref 来跟踪滚动状态，避免闭包问题
    const isAutoScrolling = useRef(false);
    const resizeObserverRef = useRef<ResizeObserver | null>(null);
    const autoScrollTimeoutRef = useRef<number | null>(null);
    const pendingSmartScrollRef = useRef<{
        forceScroll: boolean;
        behaviorOverride?: ScrollBehavior;
    } | null>(null);
    const lastUserScrollIntentAtRef = useRef<number | null>(null);
    const stableScrollFrameRef = useRef<number | null>(null);
    const smartScrollRef = useRef<UseScrollManagementReturn["smartScroll"] | null>(
        null,
    );

    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        const syncViewportHeightVar = () => {
            const { viewportHeight, liveOnlyHeight, historyTailHeight } =
                computeChatViewportHeights(container);

            // 兼容旧逻辑：接近整段滚动视口
            container.style.setProperty(
                CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR,
                `${viewportHeight}px`,
            );
            container.style.setProperty(
                CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR,
                `${liveOnlyHeight}px`,
            );
            container.style.setProperty(
                CHAT_SCROLL_HISTORY_TAIL_VIEWPORT_HEIGHT_CSS_VAR,
                `${historyTailHeight}px`,
            );
        };

        syncViewportHeightVar();

        const resizeObserver = new ResizeObserver(() => {
            syncViewportHeightVar();
        });
        resizeObserver.observe(container);

        return () => {
            resizeObserver.disconnect();
            container.style.removeProperty(CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR);
            container.style.removeProperty(
                CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR,
            );
            container.style.removeProperty(
                CHAT_SCROLL_HISTORY_TAIL_VIEWPORT_HEIGHT_CSS_VAR,
            );
        };
    }, []);

    const queuePendingSmartScroll = useCallback(
        (forceScroll: boolean, behaviorOverride?: ScrollBehavior) => {
            const existing = pendingSmartScrollRef.current;
            pendingSmartScrollRef.current = {
                forceScroll: existing?.forceScroll || forceScroll,
                behaviorOverride:
                    behaviorOverride ?? existing?.behaviorOverride,
            };
        },
        [],
    );

    const clearAutoScrollTimeout = useCallback(() => {
        if (autoScrollTimeoutRef.current !== null) {
            window.clearTimeout(autoScrollTimeoutRef.current);
            autoScrollTimeoutRef.current = null;
        }
    }, []);

    const releaseAutoScrolling = useCallback(() => {
        clearAutoScrollTimeout();
        isAutoScrolling.current = false;

        const pending = pendingSmartScrollRef.current;
        pendingSmartScrollRef.current = null;
        if (!pending) {
            return;
        }

        requestAnimationFrame(() => {
            smartScrollRef.current?.(
                pending.forceScroll,
                pending.behaviorOverride,
            );
        });
    }, [clearAutoScrollTimeout]);

    const scheduleAutoScrollRelease = useCallback(
        (delayMs: number) => {
            clearAutoScrollTimeout();
            autoScrollTimeoutRef.current = window.setTimeout(() => {
                autoScrollTimeoutRef.current = null;
                releaseAutoScrolling();
            }, delayMs);
        },
        [clearAutoScrollTimeout, releaseAutoScrolling],
    );

    const hasRecentUserScrollIntent = useCallback(() => {
        return (
            lastUserScrollIntentAtRef.current !== null
            && performance.now() - lastUserScrollIntentAtRef.current
                < USER_SCROLL_SUPPRESSION_MS
        );
    }, []);

    const handleUserScrollIntent = useCallback((
        event?: WheelEvent<HTMLDivElement> | TouchEvent<HTMLDivElement>,
    ) => {
        lastUserScrollIntentAtRef.current = performance.now();
        pendingSmartScrollRef.current = null;
        clearAutoScrollTimeout();
        isAutoScrolling.current = false;
        const container = scrollContainerRef.current;
        if (container) {
            const distanceToBottom =
                container.scrollHeight - container.scrollTop - container.clientHeight;
            const isWheelUp =
                !!event && "deltaY" in event && event.deltaY < 0;
            if (isWheelUp || distanceToBottom >= 10) {
                isUserScrolledUpRef.current = true;
            }
        }
        if (resizeObserverRef.current) {
            resizeObserverRef.current.disconnect();
            resizeObserverRef.current = null;
        }
    }, [clearAutoScrollTimeout]);

    const syncScrollState = useCallback((container?: HTMLDivElement | null) => {
        const target = container ?? scrollContainerRef.current;
        if (!target) {
            return;
        }

        const { scrollTop, scrollHeight, clientHeight } = target;
        const atBottom = scrollHeight - scrollTop - clientHeight < 10;
        isUserScrolledUpRef.current = !atBottom;
    }, []);

    // 处理用户滚动事件
    const handleScroll = useCallback(() => {
        // 如果是程序触发的自动滚动，则忽略此次事件
        if (isAutoScrolling.current) {
            return;
        }

        syncScrollState();
    }, [syncScrollState]);

    // 智能滚动函数
    const smartScroll = useCallback((forceScroll: boolean = false, behaviorOverride?: ScrollBehavior) => {
        if (isChatScrollPerfProbeActive()) {
            return;
        }
        if (hasRecentUserScrollIntent()) {
            return;
        }

        // 如果当前正处于程序触发的平滑滚动阶段，避免用 auto 覆盖动画
        if (isAutoScrolling.current) {
            queuePendingSmartScroll(forceScroll, behaviorOverride);
            return;
        }

        // 从 Ref 读取状态，这总是最新的值
        if ((!forceScroll && isUserScrolledUpRef.current) || !scrollContainerRef.current) {
            return;
        }

        const container = scrollContainerRef.current;
        if (!container) return;

        // 清理之前的观察器
        if (resizeObserverRef.current) {
            resizeObserverRef.current.disconnect();
        }

        const scrollToBottom = () => {
            // 再次从 Ref 检查，确保万无一失
            if (isAutoScrolling.current || (!forceScroll && isUserScrolledUpRef.current) || !scrollContainerRef.current) {
                if (isAutoScrolling.current) {
                    queuePendingSmartScroll(forceScroll, behaviorOverride);
                }
                if (resizeObserverRef.current) {
                    resizeObserverRef.current.disconnect();
                }
                return;
            }
            const c = scrollContainerRef.current!;
            const distanceToBottom = c.scrollHeight - c.scrollTop - c.clientHeight;
            // 优先使用外部传入的行为；否则距离较大时使用平滑滚动，小距离使用 auto
            const behavior: ScrollBehavior = behaviorOverride ?? (distanceToBottom > 120 ? 'smooth' : 'auto');

            lastUserScrollIntentAtRef.current = null;
            isAutoScrolling.current = true;
            c.scrollTo({ top: c.scrollHeight, behavior });
            if (resizeObserverRef.current) {
                resizeObserverRef.current.disconnect();
            }
            scheduleAutoScrollRelease(
                behavior === "smooth"
                    ? SMOOTH_SCROLL_LOCK_MS
                    : AUTO_SCROLL_LOCK_MS,
            );
        };

        // 优先观察最后一组容器，其次观察最后一条消息元素
        const lastReplyContainer = container.querySelector('#last-reply-container') as HTMLElement | null;
        const messageItems = disableTailObservation
            ? []
            : container.querySelectorAll('[data-message-item]');
        const lastMessageItem = (messageItems.length > 0
            ? (messageItems[messageItems.length - 1] as HTMLElement)
            : null);
        const observed: Element | null = lastReplyContainer || lastMessageItem || (disableTailObservation ? null : container.lastElementChild);

        if (observed) {
            resizeObserverRef.current = new ResizeObserver(() => {
                scrollToBottom();
            });
            resizeObserverRef.current.observe(observed);
        }

        // 回退：若 ResizeObserver 未触发，下一帧也滚动一次
        requestAnimationFrame(() => scrollToBottom());
    }, [disableTailObservation, hasRecentUserScrollIntent, queuePendingSmartScroll, scheduleAutoScrollRelease]);

    smartScrollRef.current = smartScroll;

    // 发送用户消息后滚动到最后一组消息所在的底部位置，利用 min-height 占位让用户消息贴近顶部
    const scrollToUserMessage = useCallback(() => {
        if (isChatScrollPerfProbeActive()) {
            return;
        }
        const container = scrollContainerRef.current;
        if (!container) return;

        lastUserScrollIntentAtRef.current = null;
        isAutoScrolling.current = true;
        // 重置用户滚动状态
        isUserScrolledUpRef.current = false;
        
        // 直接滚动到容器底部（最底部位置）
        container.scrollTo({
            top: container.scrollHeight,
            behavior: 'smooth'
        });

        scheduleAutoScrollRelease(SMOOTH_SCROLL_LOCK_MS);
    }, [scheduleAutoScrollRelease]);

    // 虚拟化列表专用：发送消息后把内容稳定滚到底部。
    // 虚拟化列表在插入新消息 / 尾部占位增长后，scrollHeight 需要多帧才稳定，
    // 一次性滚动会够不到底。这里用有界的多帧循环持续钉到底，直到高度稳定或超时，
    // 绕开 smartScroll 里锁 + ResizeObserver 的竞态。
    const scrollToBottomStable = useCallback(() => {
        if (isChatScrollPerfProbeActive()) {
            return;
        }
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }

        // 取消已有的观察器/锁/待执行滚动，避免相互干扰
        if (resizeObserverRef.current) {
            resizeObserverRef.current.disconnect();
            resizeObserverRef.current = null;
        }
        clearAutoScrollTimeout();
        pendingSmartScrollRef.current = null;
        if (stableScrollFrameRef.current !== null) {
            cancelAnimationFrame(stableScrollFrameRef.current);
            stableScrollFrameRef.current = null;
        }

        lastUserScrollIntentAtRef.current = null;
        isUserScrolledUpRef.current = false;
        isAutoScrolling.current = true;

        // —— 可调参数 ——
        // 连续多少帧 maxScrollTop 不变（≤1px）才算稳定
        const STABLE_FRAME_COUNT = 16;
        // 观察到高度增长后，最少再钉底多少帧才允许早退
        const MIN_FRAME_AFTER_GROWTH = 16;
        // 一直没观察到高度增长（说明布局在调用时已就绪）时，等这么多帧再收手
        const NO_GROWTH_SETTLE_FRAME_COUNT = 48;
        // 硬上限：无论如何最多钉这么久，兜底防止死循环
        const MAX_DURATION_MS = 2000;

        let elapsedFrames = 0;
        let framesSinceGrowth = 0;
        let stableFrames = 0;
        let lastMaxScrollTop = -1;
        let baselineMaxScrollTop = -1;
        let hasGrown = false;
        const startedAt = performance.now();

        const step = () => {
            stableScrollFrameRef.current = null;
            const target = scrollContainerRef.current;
            if (!target) {
                isAutoScrolling.current = false;
                return;
            }
            // 用户在此期间主动滚动则让位
            if (hasRecentUserScrollIntent()) {
                isAutoScrolling.current = false;
                return;
            }

            const maxScrollTop = Math.max(
                0,
                target.scrollHeight - target.clientHeight,
            );
            target.scrollTop = maxScrollTop;
            elapsedFrames += 1;

            if (baselineMaxScrollTop < 0) {
                baselineMaxScrollTop = maxScrollTop;
            }
            // 关键：只有真正看到高度长大之后，才认为“内容已就位”，
            // 避免把发送初期还没测量的旧小高度误判成稳定而过早停下
            if (maxScrollTop > baselineMaxScrollTop + 1) {
                hasGrown = true;
                framesSinceGrowth = 0;
            }

            if (Math.abs(maxScrollTop - lastMaxScrollTop) <= 1) {
                stableFrames += 1;
            } else {
                stableFrames = 0;
            }
            lastMaxScrollTop = maxScrollTop;
            if (hasGrown) {
                framesSinceGrowth += 1;
            }

            const settledAfterGrowth =
                hasGrown
                && framesSinceGrowth >= MIN_FRAME_AFTER_GROWTH
                && stableFrames >= STABLE_FRAME_COUNT;
            const settledWithoutGrowth =
                !hasGrown
                && elapsedFrames >= NO_GROWTH_SETTLE_FRAME_COUNT
                && stableFrames >= STABLE_FRAME_COUNT;
            const timedOut = performance.now() - startedAt >= MAX_DURATION_MS;

            if (settledAfterGrowth || settledWithoutGrowth || timedOut) {
                isAutoScrolling.current = false;
                return;
            }

            stableScrollFrameRef.current = requestAnimationFrame(step);
        };

        stableScrollFrameRef.current = requestAnimationFrame(step);
    }, [clearAutoScrollTimeout, hasRecentUserScrollIntent]);

    // 组件卸载时清理资源
    useEffect(() => {
        return () => {
            clearAutoScrollTimeout();
            if (resizeObserverRef.current) {
                resizeObserverRef.current.disconnect();
                resizeObserverRef.current = null;
            }
            if (stableScrollFrameRef.current !== null) {
                cancelAnimationFrame(stableScrollFrameRef.current);
                stableScrollFrameRef.current = null;
            }
        };
    }, [clearAutoScrollTimeout]);

    return {
        messagesEndRef,
        scrollContainerRef,
        handleScroll,
        handleUserScrollIntent,
        syncScrollState,
        smartScroll,
        scrollToUserMessage,
        scrollToBottomStable,
    };
}
