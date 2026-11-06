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

/**
 * 发送后最后一轮贴底时，user 气泡距滚动视口顶部的固定距离。
 * 桌面 / 移动端 padding 不同，必须用同一个常量反推 min-height，两端位置才一致。
 */
export const LAST_REPLY_TOP_OFFSET_PX = 8;

// 滚动容器可视高度相对实际高度预留的缓冲，避免占位刚好等于视口导致临界抖动。
const CHAT_SCROLL_VIEWPORT_HEIGHT_SAFETY_PX = 10;

export interface ChatViewportHeights {
    // 完整可视高度（扣掉安全缓冲）
    viewportHeight: number;
    // last-reply min-height：贴底时 user 气泡顶部固定距视口顶 LAST_REPLY_TOP_OFFSET_PX
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
    const endAnchor = container.querySelector(
        "[data-aipp-slot='chat-messages-end-anchor']",
    );
    const endAnchorHeight =
        endAnchor instanceof HTMLElement ? endAnchor.offsetHeight : 0;
    // last-reply 与 end-anchor 之间的 flex 间距
    const rowGap = Number.parseFloat(style.rowGap) || 0;

    const viewportHeight = Math.max(
        0,
        container.clientHeight - CHAT_SCROLL_VIEWPORT_HEIGHT_SAFETY_PX,
    );

    return {
        viewportHeight,
        // clientHeight 已不含 border。贴底时 last-reply 顶部 = clientHeight − paddingBottom − rowGap − endAnchor − minHeight。
        // 反推 minHeight 让顶部固定落在 LAST_REPLY_TOP_OFFSET_PX，而不是随 paddingTop 变化
        //（旧算法得到 paddingTop − rowGap：桌面 8px，移动端 p-3 时为 −4px，被切掉一截）。
        liveOnlyHeight: Math.max(
            0,
            container.clientHeight
                - paddingBottom
                - rowGap
                - endAnchorHeight
                - LAST_REPLY_TOP_OFFSET_PX,
        ),
        historyTailHeight: Math.max(0, viewportHeight - paddingTop - 16),
    };
}
