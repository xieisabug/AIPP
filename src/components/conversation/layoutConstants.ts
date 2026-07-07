export const CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR =
    "--aipp-chat-scroll-viewport-height";

export const CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR =
    "--aipp-chat-live-only-viewport-height";

// 有历史消息时的尾部占位高度：只扣掉滚动容器的顶部 padding，
// 以便把历史顶出屏幕（"最后一屏只显示当前轮次"），同时不切掉 user 消息顶部。
export const CHAT_SCROLL_HISTORY_TAIL_VIEWPORT_HEIGHT_CSS_VAR =
    "--aipp-chat-history-tail-viewport-height";

/**
 * 最后一轮对话的 min-height：必须用「扣除 padding/border/end-anchor 后」的 live-only 高度。
 * 若直接用 clientHeight 系变量，贴底滚动时会把 user 气泡顶出可视区
 *（总管家等嵌套 flex 布局上更明显）。
 */
export const LAST_REPLY_CONTAINER_MIN_HEIGHT = `var(${CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR}, var(${CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR}, 0px))`;

export const LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX = 120;

// 滚动容器可视高度相对实际高度预留的缓冲，避免占位刚好等于视口导致临界抖动。
const CHAT_SCROLL_VIEWPORT_HEIGHT_SAFETY_PX = 10;

export interface ChatViewportHeights {
    // 完整可视高度（扣掉安全缓冲）
    viewportHeight: number;
    // last-reply min-height：内容盒高度 − end-anchor，不扣 rowGap
    liveOnlyHeight: number;
    // 有历史时尾部占位高度：仅扣掉顶部 padding，把历史顶出屏幕又不切掉 user 消息
    historyTailHeight: number;
}

// 统一计算聊天滚动区域的各种占位高度，作为唯一数据源，
// 供 useScrollManagement 设置 CSS 变量与 VirtuosoMessageList 直接计算像素值共用。
export function computeChatViewportHeights(
    container: HTMLElement,
): ChatViewportHeights {
    const style = window.getComputedStyle(container);
    const paddingTop = Number.parseFloat(style.paddingTop) || 0;
    const paddingBottom = Number.parseFloat(style.paddingBottom) || 0;
    const borderTop = Number.parseFloat(style.borderTopWidth) || 0;
    const borderBottom = Number.parseFloat(style.borderBottomWidth) || 0;
    // border-box 下 flex 子项可用高度 = clientHeight - 垂直 padding - border
    const contentBoxHeight = Math.max(
        0,
        container.clientHeight
            - paddingTop
            - paddingBottom
            - borderTop
            - borderBottom,
    );
    const endAnchor = container.querySelector(
        "[data-aipp-slot='chat-messages-end-anchor']",
    );
    const endAnchorHeight =
        endAnchor instanceof HTMLElement ? endAnchor.offsetHeight : 0;

    const viewportHeight = Math.max(
        0,
        container.clientHeight - CHAT_SCROLL_VIEWPORT_HEIGHT_SAFETY_PX,
    );

    return {
        viewportHeight,
        // last-reply min-height：内容区 − end-anchor。
        // 不扣 rowGap：gap 由 flex 布局在子项外占用，再扣会少占约 16px，
        // 贴底后 user 上方会多出一条空隙。
        liveOnlyHeight: Math.max(0, contentBoxHeight - endAnchorHeight),
        historyTailHeight: Math.max(0, viewportHeight - paddingTop - 16),
    };
}
