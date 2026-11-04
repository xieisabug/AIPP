import { VIRTUAL_ROW_GAP_PX } from "./virtualizedMessageListLayout";

type ShiningMessageSetter =
    | Set<number>
    | ((prev: Set<number>) => Set<number>);

/** center：消息居中（搜索/引用定位）；start：消息顶到视口顶部（轮次导航）。 */
export type MessageScrollAlign = "center" | "start";

/**
 * start 对齐时目标上方保留的空隙。必须小于消息行间距，
 * 否则上一条消息的底边框会露出在视口顶部。
 */
export const MESSAGE_SCROLL_START_GAP_PX = VIRTUAL_ROW_GAP_PX / 2;

/** 将消息滚入视口；start 对齐需要滚动容器来精确扣除顶部空隙。 */
export function scrollMessageIntoView(
    target: HTMLElement,
    align: MessageScrollAlign,
    container?: HTMLElement | null,
    behavior: ScrollBehavior = "smooth",
) {
    if (align === "start" && container) {
        const top = container.scrollTop
            + target.getBoundingClientRect().top
            - container.getBoundingClientRect().top
            - MESSAGE_SCROLL_START_GAP_PX;
        container.scrollTo({ top: Math.max(0, top), behavior });
        return;
    }
    target.scrollIntoView({ behavior, block: align });
}

interface ScrollHighlightOptions {
    target: HTMLElement;
    messageId: number;
    setShiningMessageIds: (value: ShiningMessageSetter) => void;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    align?: MessageScrollAlign;
    container?: HTMLElement | null;
    requestFrame?: (callback: FrameRequestCallback) => number;
    scheduleTimeout?: (callback: () => void, delayMs: number) => ReturnType<typeof setTimeout>;
}

export function applyScrollHighlight({
    target,
    messageId,
    setShiningMessageIds,
    clearPendingScrollMessageId,
    align = "center",
    container,
    requestFrame = requestAnimationFrame,
    scheduleTimeout = (callback, delayMs) => setTimeout(callback, delayMs),
}: ScrollHighlightOptions) {
    requestFrame(() => {
        scrollMessageIntoView(target, align, container);
        setShiningMessageIds(() => new Set([messageId]));
        scheduleTimeout(() => {
            setShiningMessageIds(new Set());
        }, 2000);
        clearPendingScrollMessageId(null);
    });
}
