import { useRef, useCallback, useEffect } from "react";

export interface UseScrollManagementReturn {
    messagesEndRef: React.RefObject<HTMLDivElement | null>;
    scrollContainerRef: React.RefObject<HTMLDivElement | null>;
    handleScroll: () => void;
    smartScroll: (forceScroll?: boolean) => void;
    scrollToUserMessage: () => void;
}

export function useScrollManagement(): UseScrollManagementReturn {
    // 滚动相关状态和逻辑
    const messagesEndRef = useRef<HTMLDivElement | null>(null);
    const scrollContainerRef = useRef<HTMLDivElement | null>(null);
    const isUserScrolledUpRef = useRef(false); // 使用 Ref 来跟踪滚动状态，避免闭包问题
    const isAutoScrolling = useRef(false);
    const resizeObserverRef = useRef<ResizeObserver | null>(null);

    // 处理用户滚动事件
    const handleScroll = useCallback(() => {
        // 如果是程序触发的自动滚动，则忽略此次事件
        if (isAutoScrolling.current) {
            return;
        }

        const container = scrollContainerRef.current;
        if (container) {
            const { scrollTop, scrollHeight, clientHeight } = container;
            // 判断是否滚动到了底部，留出 10px 的容差
            const atBottom = scrollHeight - scrollTop - clientHeight < 10;

            // 直接更新 Ref 的值
            isUserScrolledUpRef.current = !atBottom;
        }
    }, []); // 依赖项为空，函数是稳定的

    // 智能滚动函数
    const smartScroll = useCallback((forceScroll: boolean = false) => {
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
            if ((!forceScroll && isUserScrolledUpRef.current) || !scrollContainerRef.current) {
                if (resizeObserverRef.current) {
                    resizeObserverRef.current.disconnect();
                }
                return;
            }
            const c = scrollContainerRef.current;
            isAutoScrolling.current = true;
            // 流式更新使用即时滚动以保持跟随性
            c!.scrollTo({ top: c!.scrollHeight, behavior: 'auto' });
            if (resizeObserverRef.current) {
                resizeObserverRef.current.disconnect();
            }
            setTimeout(() => {
                isAutoScrolling.current = false;
            }, 100);
        };

        // 优先观察最后一组容器，其次观察最后一条消息元素
        const lastReplyContainer = container.querySelector('#last-reply-container') as HTMLElement | null;
        const messageItems = container.querySelectorAll('[data-message-item]');
        const lastMessageItem = (messageItems.length > 0
            ? (messageItems[messageItems.length - 1] as HTMLElement)
            : null);
        const observed: Element | null = lastReplyContainer || lastMessageItem || container.lastElementChild;

        if (observed) {
            resizeObserverRef.current = new ResizeObserver(() => {
                scrollToBottom();
            });
            resizeObserverRef.current.observe(observed);
        }

        // 回退：若 ResizeObserver 未触发，下一帧也滚动一次
        requestAnimationFrame(() => scrollToBottom());
    }, []); // 依赖项为空，函数是稳定的

    // 将最后的用户消息滚动到视口顶部（ChatGPT风格）
    const scrollToUserMessage = useCallback(() => {
        const container = scrollContainerRef.current;
        if (!container) return;

        isAutoScrolling.current = true;
        // 重置用户滚动状态
        isUserScrolledUpRef.current = false;
        
        // 直接滚动到容器底部（最底部位置）
        container.scrollTo({
            top: container.scrollHeight,
            behavior: 'smooth'
        });

        setTimeout(() => {
            isAutoScrolling.current = false;
        }, 500); // 增加延迟以匹配平滑滚动时间
    }, []);

    // 组件卸载时清理资源
    useEffect(() => {
        return () => {
            if (resizeObserverRef.current) {
                resizeObserverRef.current.disconnect();
                resizeObserverRef.current = null;
            }
        };
    }, []); // 只在组件卸载时清理

    return {
        messagesEndRef,
        scrollContainerRef,
        handleScroll,
        smartScroll,
        scrollToUserMessage,
    };
}
