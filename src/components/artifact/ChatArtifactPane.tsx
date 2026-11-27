import { useCallback, useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import EmbeddedArtifactPreview from "@/components/EmbeddedArtifactPreview";
import { WindowControls } from "@/components/window/WindowTitleBar";
import { ARTIFACT_PANE_WINDOW, closeArtifactPane } from "./artifactPane";

const WIDTH_KEY = "aipp.chat.artifactPaneWidth";
const DEFAULT_WIDTH = 520;
const MIN_WIDTH = 360;
/** 给会话区至少留出的宽度。 */
const MIN_CONVERSATION_WIDTH = 420;

/** 分栏宽度限制在 [MIN_WIDTH, 可用宽度 - 会话区最小宽度] 之间。 */
export function clampPaneWidth(width: number, available: number): number {
    const max = Math.max(MIN_WIDTH, available - MIN_CONVERSATION_WIDTH);
    return Math.round(Math.min(Math.max(width, MIN_WIDTH), max));
}

function readStoredWidth(): number {
    const stored = Number(localStorage.getItem(WIDTH_KEY));
    return Number.isFinite(stored) && stored > 0 ? stored : DEFAULT_WIDTH;
}

/** 聊天窗口右侧的 Artifact 预览分栏：左边缘可拖动调宽，宽度记在本机。 */
export default function ChatArtifactPane() {
    const rootRef = useRef<HTMLDivElement | null>(null);
    const [width, setWidth] = useState(readStoredWidth);
    const widthRef = useRef(width);
    widthRef.current = width;
    const [dragging, setDragging] = useState(false);

    const available = useCallback(() => rootRef.current?.parentElement?.clientWidth ?? window.innerWidth, []);

    // 窗口变窄时收回宽度
    useEffect(() => {
        const fit = () => setWidth((current) => clampPaneWidth(current, available()));
        fit();
        window.addEventListener("resize", fit);
        return () => window.removeEventListener("resize", fit);
    }, [available]);

    const startResize = useCallback(
        (event: ReactPointerEvent<HTMLDivElement>) => {
            event.preventDefault();
            const handle = event.currentTarget;
            handle.setPointerCapture(event.pointerId);
            const startX = event.clientX;
            const startWidth = widthRef.current;
            setDragging(true);
            const move = (moveEvent: PointerEvent) => {
                setWidth(clampPaneWidth(startWidth + (startX - moveEvent.clientX), available()));
            };
            const end = () => {
                handle.removeEventListener("pointermove", move);
                handle.removeEventListener("pointerup", end);
                handle.removeEventListener("pointercancel", end);
                setDragging(false);
                localStorage.setItem(WIDTH_KEY, String(widthRef.current));
            };
            handle.addEventListener("pointermove", move);
            handle.addEventListener("pointerup", end);
            handle.addEventListener("pointercancel", end);
        },
        [available],
    );

    const popOut = useCallback(() => {
        invoke("pop_out_artifact_preview", { sourceWindow: ARTIFACT_PANE_WINDOW })
            .then(() => closeArtifactPane())
            .catch((error) => toast.error(`在预览窗口打开失败：${String(error)}`));
    }, []);

    return (
        <div ref={rootRef} className="relative flex h-full shrink-0" style={{ width }} data-aipp-slot="chat-artifact-pane">
            <div
                role="separator"
                aria-orientation="vertical"
                aria-label="调整预览宽度"
                className={`absolute -left-1 top-0 z-10 h-full w-2 cursor-col-resize ${dragging ? "bg-border" : "hover:bg-border/60"}`}
                onPointerDown={startResize}
            />
            {/* 拖动时盖住预览 iframe，否则指针进入 iframe 后收不到移动事件 */}
            {dragging && <div className="absolute inset-0 z-10 cursor-col-resize" />}
            <div className="flex min-w-0 flex-1 flex-col border-l border-border">
                <EmbeddedArtifactPreview
                    className="flex-1"
                    windowLabel={ARTIFACT_PANE_WINDOW}
                    stopOnWindowClose={false}
                    onOpenInWindow={popOut}
                    onClose={closeArtifactPane}
                    trailing={<WindowControls />}
                />
            </div>
        </div>
    );
}
