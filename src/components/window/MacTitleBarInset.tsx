import { usePlatform } from "@/hooks/use-platform";
import { cn } from "@/utils/utils";

/**
 * macOS 主窗口使用 Overlay 标题栏（见 `src-tauri/src/window.rs` 的
 * `with_macos_overlay_titlebar`），红黄绿按钮悬浮在网页内容之上。
 * 该高度需与 Rust 侧 `traffic_light_position` 的 y 值配合，让按钮垂直居中。
 */
export const MAC_TITLEBAR_INSET_HEIGHT = 36;

/** 当前窗口是否使用 macOS Overlay 标题栏（仅 macOS 桌面端）。 */
export function useMacOverlayTitleBar(): boolean {
    return usePlatform() === "macos";
}

/**
 * 侧边栏顶部的拖拽条：为红黄绿按钮留出位置，同时承担窗口拖动。
 * 背景透明，沿用所在侧边栏的底色，让按钮与界面融为一体。非 macOS 不渲染。
 */
export function MacTitleBarInset({ className }: { className?: string }) {
    const enabled = useMacOverlayTitleBar();
    if (!enabled) return null;
    return (
        <div
            aria-hidden
            data-tauri-drag-region
            data-aipp-slot="mac-titlebar-inset"
            className={cn("shrink-0 select-none", className)}
            style={{ height: MAC_TITLEBAR_INSET_HEIGHT }}
        />
    );
}
