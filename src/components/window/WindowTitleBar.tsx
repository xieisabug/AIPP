import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Minus, Square, X } from "lucide-react";
import { usePlatform } from "@/hooks/use-platform";
import { cn } from "@/utils/utils";

/**
 * 桌面主窗口（聊天 / 管家 / 设置）的标题栏布局，三个桌面平台尽量长得一样：
 *
 * - macOS：Overlay 标题栏（`src-tauri/src/window.rs` 的 `with_macos_overlay_titlebar`），
 *   红黄绿按钮悬浮在侧边栏第一行左侧，顶栏内容从 `MAC_TRAFFIC_LIGHTS_SAFE_LEFT` 开始；
 * - Windows：去掉系统标题栏（`with_windows_custom_titlebar`），最小化 / 最大化 / 关闭由
 *   `WindowControls` 画在窗口右上角，与 macOS 左上角的红黄绿按钮镜像：能确定所在头部时
 *   直接排进头部（聊天窗口），否则用 `FloatingWindowControls` 固定在右上角、头部用
 *   `useWindowControlsInset` 留位；
 * - Linux：保留系统标题栏（无边框窗口在部分桌面环境下缩放边缘不可靠），只统一侧边栏布局。
 *
 * 三个窗口的侧栏标题栏行与右侧头部同高（`TITLEBAR_HEIGHT` = `WINDOW_HEADER_HEIGHT`），
 * 红黄绿按钮 / 窗口控制按钮与头部操作按钮同在中线 34（Rust 侧 `MAC_TITLEBAR_CENTER_Y`）。
 */
export const TITLEBAR_HEIGHT = 68;
/** 右侧会话头部（聊天标题栏、总管家头部、详情栏头部）的统一高度。 */
export const WINDOW_HEADER_HEIGHT = TITLEBAR_HEIGHT;
export const MAC_TRAFFIC_LIGHTS_SAFE_LEFT = 92;
/** 没有红黄绿按钮时，顶栏左侧只留常规内边距。 */
const TITLEBAR_PADDING_LEFT = 12;
/** `WindowControls` 宽度：三个 32px 按钮 + 两个 2px 间距。 */
export const WINDOW_CONTROLS_WIDTH = 100;
/** 头部操作按钮与窗口控制按钮之间的间距。 */
const WINDOW_CONTROLS_GAP = 12;

/** macOS Overlay 标题栏：侧边栏第一行左侧要让出红黄绿按钮。 */
export function useMacOverlayTitleBar(): boolean {
    return usePlatform() === "macos";
}

/** Windows 无边框窗口：页面自己画最小化 / 最大化 / 关闭。 */
export function useCustomWindowControls(): boolean {
    return usePlatform() === "windows";
}

/** 没有系统标题栏（macOS / Windows）：页面顶部空白处承担窗口拖动。 */
export function useFramelessTitleBar(): boolean {
    const platform = usePlatform();
    return platform === "macos" || platform === "windows";
}

/** 桌面统一布局：侧边栏第一行是标题栏行（主操作），第二行是 Logo 与工具按钮。 */
export function useWindowTitleBarLayout(): boolean {
    const platform = usePlatform();
    return platform === "macos" || platform === "windows" || platform === "linux";
}

/**
 * 与 `WindowControls` 同排的头部右侧留位：`basePadding` 是头部原有的右内边距，
 * 控制按钮放在这段内边距的内侧。非 Windows 返回 undefined，保持原样。
 */
export function useWindowControlsInset(basePadding: number): CSSProperties | undefined {
    return useCustomWindowControls()
        ? { paddingRight: basePadding + WINDOW_CONTROLS_WIDTH + WINDOW_CONTROLS_GAP }
        : undefined;
}

interface WindowTitleBarProps {
    className?: string;
    /** 与窗口控制按钮同排、靠右排列的操作按钮；为空时只作为按钮位置与拖拽条。 */
    children?: ReactNode;
    "data-aipp-slot"?: string;
}

/**
 * 侧边栏顶栏：右侧放置该栏的主操作，空白处承担窗口拖动（`data-tauri-drag-region`
 * 只对按下的元素本身生效，子按钮照常点击）。macOS 左侧留给红黄绿按钮。
 * 背景默认透明，沿用所在侧边栏的底色。非桌面平台不渲染。
 */
export function WindowTitleBar({ className, children, "data-aipp-slot": slot = "window-titlebar" }: WindowTitleBarProps) {
    const enabled = useWindowTitleBarLayout();
    const mac = useMacOverlayTitleBar();
    if (!enabled) return null;
    return (
        <div
            data-tauri-drag-region
            data-aipp-slot={slot}
            className={cn("flex shrink-0 select-none items-center justify-end gap-0.5 pr-2", className)}
            style={{ height: TITLEBAR_HEIGHT, paddingLeft: mac ? MAC_TRAFFIC_LIGHTS_SAFE_LEFT : TITLEBAR_PADDING_LEFT }}
        >
            {children}
        </div>
    );
}

const controlButtonClass =
    "inline-flex size-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring";

/** 当前窗口是否最大化（随窗口缩放同步），用于切换「最大化 / 还原」图标。 */
function useWindowMaximized(enabled: boolean): boolean {
    const [maximized, setMaximized] = useState(false);
    useEffect(() => {
        if (!enabled) return;
        let appWindow: ReturnType<typeof getCurrentWindow>;
        try {
            appWindow = getCurrentWindow();
        } catch {
            return; // 没有 Tauri 窗口上下文（测试 / 预览）时只显示按钮
        }
        let cancelled = false;
        let unlisten: (() => void) | undefined;
        const sync = () => {
            appWindow.isMaximized().then((value) => {
                if (!cancelled) setMaximized(!!value);
            }).catch(() => {});
        };
        sync();
        appWindow.onResized(sync).then((fn) => {
            if (cancelled) fn();
            else unlisten = fn;
        }).catch(() => {});
        return () => {
            cancelled = true;
            unlisten?.();
        };
    }, [enabled]);
    return maximized;
}

/**
 * Windows 窗口控制按钮（最小化 / 最大化 / 关闭），按普通元素排在所在头部的行内，
 * 出现时淡入（同一窗口里可能随布局在不同头部间切换，例如聊天窗口的详情栏展开 / 收起）。
 * 关闭走 `close()`，与系统关闭按钮一样触发 CloseRequested（主窗口会被隐藏而不是退出）。
 * 其他平台不渲染。
 */
export function WindowControls({ className }: { className?: string }) {
    const enabled = useCustomWindowControls();
    const maximized = useWindowMaximized(enabled);
    if (!enabled) return null;
    const appWindow = () => getCurrentWindow();
    return (
        <div className={cn("aipp-window-controls-enter flex shrink-0 select-none items-center gap-0.5", className)} data-aipp-slot="window-controls">
            <button type="button" className={controlButtonClass} aria-label="最小化" title="最小化"
                onClick={() => void appWindow().minimize()} data-aipp-slot="window-controls-minimize">
                <Minus className="size-4" strokeWidth={1.5} />
            </button>
            <button type="button" className={controlButtonClass} aria-label={maximized ? "还原" : "最大化"} title={maximized ? "还原" : "最大化"}
                onClick={() => void appWindow().toggleMaximize()} data-aipp-slot="window-controls-maximize">
                {maximized ? <Copy className="size-3.5 -scale-x-100" strokeWidth={1.5} /> : <Square className="size-3.5" strokeWidth={1.5} />}
            </button>
            <button type="button" className={cn(controlButtonClass, "hover:bg-destructive hover:text-white")} aria-label="关闭" title="关闭"
                onClick={() => void appWindow().close()} data-aipp-slot="window-controls-close">
                <X className="size-4" strokeWidth={1.5} />
            </button>
        </div>
    );
}

interface FloatingWindowControlsProps {
    /** 按钮中线距窗口顶部的距离，与所在头部的操作按钮对齐。 */
    centerY: number;
    /** 距窗口右缘的距离，等于所在头部的右内边距（含外层卡片的外边距）。 */
    right: number;
}

/**
 * 固定在窗口右上角的窗口控制按钮：用于头部有多种形态、或某些状态下没有头部的窗口
 * （管家、设置、窄屏布局），所在头部用 `useWindowControlsInset` 留位。
 */
export function FloatingWindowControls({ centerY, right }: FloatingWindowControlsProps) {
    if (!useCustomWindowControls()) return null;
    return (
        <div className="fixed z-40" style={{ top: centerY - 16, right }} data-aipp-slot="window-controls-floating">
            <WindowControls />
        </div>
    );
}
