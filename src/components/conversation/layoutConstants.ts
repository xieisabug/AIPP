export const CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR =
    "--aipp-chat-scroll-viewport-height";

export const CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR =
    "--aipp-chat-live-only-viewport-height";

/**
 * 最后一轮对话的 min-height：必须用「扣除 padding/gap 后」的 live-only 高度。
 * 若直接用 clientHeight 系变量，贴底滚动时会把 user 气泡顶出可视区
 *（总管家等嵌套 flex 布局上更明显）。
 */
export const LAST_REPLY_CONTAINER_MIN_HEIGHT = `var(${CHAT_SCROLL_LIVE_ONLY_VIEWPORT_HEIGHT_CSS_VAR}, var(${CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR}, 0px))`;

export const LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX = 120;
