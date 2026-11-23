import type { CSSProperties, ReactNode } from "react";
import { usePlatform } from "@/hooks/use-platform";
import { cn } from "@/utils/utils";

/**
 * macOS 主窗口使用 Overlay 标题栏（见 `src-tauri/src/window.rs` 的
 * `with_macos_overlay_titlebar`），红黄绿按钮悬浮在网页内容之上，与侧边栏顶栏的操作按钮同排。
 *
 * 布局约定（两侧数值需保持一致）：
 * - 按钮从 x=20 起排（`MAC_TRAFFIC_LIGHTS_X`），macOS 26 上三颗约占 61pt，再留出间距，
 *   顶栏内容从 `MAC_TRAFFIC_LIGHTS_SAFE_LEFT` 开始；
 * - 三个主窗口（聊天 / 管家 / 设置）顶栏同高 `MAC_TITLEBAR_HEIGHT`，按钮中线 26（Rust 侧
 *   `MAC_TITLEBAR_CENTER_Y`），红黄绿按钮在各窗口位置一致，并与管家主区头部同高。
 */
export const MAC_TITLEBAR_HEIGHT = 52;
export const MAC_TRAFFIC_LIGHTS_SAFE_LEFT = 92;

/** 当前窗口是否使用 macOS Overlay 标题栏（仅 macOS 桌面端）。 */
export function useMacOverlayTitleBar(): boolean {
    return usePlatform() === "macos";
}

/** 需要与红黄绿按钮同排的自定义顶栏使用的高度与左侧避让。 */
export const macTitleBarRowStyle: CSSProperties = {
    height: MAC_TITLEBAR_HEIGHT,
    paddingLeft: MAC_TRAFFIC_LIGHTS_SAFE_LEFT,
};

interface MacTitleBarProps {
    className?: string;
    /** 与红黄绿按钮同排、靠右排列的操作按钮；为空时只作为按钮位置与拖拽条。 */
    children?: ReactNode;
    "data-aipp-slot"?: string;
}

/**
 * 侧边栏顶栏：左侧留给红黄绿按钮，右侧放置该栏的操作按钮，空白处承担窗口拖动
 * （`data-tauri-drag-region` 只对按下的元素本身生效，子按钮照常点击）。
 * 背景默认透明，沿用所在侧边栏的底色。非 macOS 不渲染。
 */
export function MacTitleBar({ className, children, "data-aipp-slot": slot = "mac-titlebar" }: MacTitleBarProps) {
    const enabled = useMacOverlayTitleBar();
    if (!enabled) return null;
    return (
        <div
            data-tauri-drag-region
            data-aipp-slot={slot}
            className={cn("flex shrink-0 select-none items-center justify-end gap-0.5 pr-2", className)}
            style={macTitleBarRowStyle}
        >
            {children}
        </div>
    );
}
