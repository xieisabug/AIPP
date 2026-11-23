import { useEffect, useRef, useState, type ComponentProps, type RefObject } from "react";
import frameHeightScript from "../../../src-tauri/src/mcp/builtin_mcp/choices/frame-height.js?raw";
import { cn } from "@/utils/utils";

/** 与 document.rs / serve-build.mjs 注入的是同一份脚本。 */
export const FRAME_HEIGHT_SCRIPT = frameHeightScript;
const MESSAGE = "aipp-frame-height";

/** 只接受该 iframe 自己窗口发来的高度；其他来源一律忽略。 */
export function useFrameHeight(frame: RefObject<HTMLIFrameElement | null>) {
    const [height, setHeight] = useState<number>();
    useEffect(() => {
        function receive(event: MessageEvent) {
            const target = frame.current?.contentWindow;
            if (!target || event.source !== target || event.data?.type !== MESSAGE) return;
            const value = Number(event.data.height);
            if (Number.isFinite(value) && value >= 0) setHeight(Math.ceil(value));
        }
        window.addEventListener("message", receive);
        return () => window.removeEventListener("message", receive);
    }, [frame]);
    return height;
}

/**
 * 按内容伸缩的预览框：收到高度前用 className 里的默认高度占位，
 * 之后跟随内容，超过 75vh 时在框内滚动。
 */
export function FitFrame({
    frameRef,
    className,
    style,
    onHeightChange,
    ...props
}: ComponentProps<"iframe"> & {
    frameRef?: RefObject<HTMLIFrameElement | null>;
    onHeightChange?: (height: number) => void;
}) {
    const own = useRef<HTMLIFrameElement>(null);
    const ref = frameRef ?? own;
    const height = useFrameHeight(ref);
    const onHeightChangeRef = useRef(onHeightChange);
    onHeightChangeRef.current = onHeightChange;
    useEffect(() => {
        if (height !== undefined) onHeightChangeRef.current?.(height);
    }, [height]);
    return (
        <iframe
            {...props}
            ref={ref}
            className={cn("block min-h-16 max-h-[75vh] w-full border-0", className)}
            style={height === undefined ? style : { ...style, height }}
        />
    );
}
