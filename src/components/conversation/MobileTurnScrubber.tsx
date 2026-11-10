import {
    useCallback,
    useEffect,
    useLayoutEffect,
    useRef,
    useState,
    type PointerEvent as ReactPointerEvent,
    type RefObject,
} from "react";
import { cn } from "@/utils/utils";
import { useTurnRailTracking, type TurnRailItem } from "./ConversationTurnRail";
import { sampleTickIndices, tickOffset, turnIndexFromOffset } from "./turnRailLayout";

export interface MobileTurnScrubberProps {
    turns: TurnRailItem[];
    scrollContainerRef: RefObject<HTMLDivElement | null>;
    onSelect: (messageId: number) => void;
}

/** 每轮在滑轨上占用的理想高度；轮次多时整体压缩到容器高度的 TRACK_HEIGHT_RATIO。 */
const TURN_SLOT_PX = 18;
const TRACK_HEIGHT_RATIO = 0.6;
/** 上下额外热区，手指略超出首尾刻度仍能命中。 */
const TRACK_PADDING_PX = 16;
const HIDE_DELAY_MS = 1600;
/** 刻度间距小于此值时抽样绘制，避免长对话的滑轨糊成一整条。 */
const MIN_TICK_GAP_PX = 8;

/**
 * 移动端对话轮次滑轨。
 *
 * 与桌面左侧导航条同一套「刻度 = 提问轮次」的定位模型：
 * 用户滑动消息列表时右侧浮现刻度，当前阅读轮次高亮；按住滑轨上下拖动
 * 会实时显示目标轮次的提问片段，松手跳转。空闲后自动隐藏，不遮挡正文点击。
 */
export default function MobileTurnScrubber({
    turns,
    scrollContainerRef,
    onSelect,
}: MobileTurnScrubberProps) {
    const { containerHeight, activeTurnId, setActiveTurnId } = useTurnRailTracking(turns, scrollContainerRef);
    const [containerTop, setContainerTop] = useState(0);
    const [visible, setVisible] = useState(false);
    const [dragIndex, setDragIndex] = useState<number | null>(null);
    const trackRef = useRef<HTMLDivElement>(null);
    const hideTimerRef = useRef<number | null>(null);
    const draggingRef = useRef(false);
    const visibleRef = useRef(false);
    visibleRef.current = visible;

    const count = turns.length;
    const trackHeight = Math.min(count * TURN_SLOT_PX, containerHeight * TRACK_HEIGHT_RATIO);

    // 滑轨覆盖在根容器上，需对齐滚动区（其上方可能有恢复提示等横幅）。
    useLayoutEffect(() => {
        setContainerTop(scrollContainerRef.current?.offsetTop ?? 0);
    }, [scrollContainerRef, containerHeight]);

    const clearHideTimer = useCallback(() => {
        if (hideTimerRef.current !== null) {
            window.clearTimeout(hideTimerRef.current);
            hideTimerRef.current = null;
        }
    }, []);

    const scheduleHide = useCallback(() => {
        clearHideTimer();
        hideTimerRef.current = window.setTimeout(() => {
            hideTimerRef.current = null;
            if (!draggingRef.current) {
                setVisible(false);
            }
        }, HIDE_DELAY_MS);
    }, [clearHideTimer]);

    // 只响应用户手势滚动：流式输出带来的程序化滚动不应让滑轨闪现。
    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container) {
            return;
        }
        const reveal = () => {
            setVisible(true);
            scheduleHide();
        };
        // 惯性滚动期间 touch 已结束，靠 scroll 续期已显示的滑轨。
        const extend = () => {
            if (visibleRef.current) {
                scheduleHide();
            }
        };
        container.addEventListener("touchmove", reveal, { passive: true });
        container.addEventListener("scroll", extend, { passive: true });
        return () => {
            container.removeEventListener("touchmove", reveal);
            container.removeEventListener("scroll", extend);
        };
    }, [scrollContainerRef, scheduleHide]);

    useEffect(() => clearHideTimer, [clearHideTimer]);

    const indexFromPointer = (clientY: number) => {
        const rect = trackRef.current?.getBoundingClientRect();
        return turnIndexFromOffset(clientY - (rect?.top ?? 0), trackHeight, count);
    };

    const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
        event.preventDefault();
        event.currentTarget.setPointerCapture?.(event.pointerId);
        draggingRef.current = true;
        clearHideTimer();
        setVisible(true);
        setDragIndex(indexFromPointer(event.clientY));
    };

    const handlePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
        if (!draggingRef.current) {
            return;
        }
        const next = indexFromPointer(event.clientY);
        setDragIndex((current) => (current === next ? current : next));
    };

    const finishDrag = (commit: boolean) => {
        const index = dragIndex;
        draggingRef.current = false;
        setDragIndex(null);
        scheduleHide();
        const target = index === null ? undefined : turns[index];
        if (commit && target) {
            setActiveTurnId(target.id);
            onSelect(target.id);
        }
    };

    // 单轮对话无需定位。
    if (count < 2) {
        return null;
    }

    const activeIndex = turns.findIndex((turn) => turn.id === activeTurnId);
    const highlightIndex = dragIndex ?? activeIndex;
    const dragTurn = dragIndex === null ? null : turns[dragIndex];

    return (
        <div
            className={cn(
                "pointer-events-none absolute right-4 z-20 transition-opacity duration-200",
                visible ? "opacity-100" : "opacity-0",
            )}
            style={{ top: containerTop, height: containerHeight }}
            data-aipp-slot="chat-mobile-turn-scrubber"
        >
            <div
                role="slider"
                aria-label="对话轮次导航"
                aria-orientation="vertical"
                aria-valuemin={1}
                aria-valuemax={count}
                aria-valuenow={highlightIndex >= 0 ? highlightIndex + 1 : undefined}
                aria-valuetext={highlightIndex >= 0 ? `第 ${highlightIndex + 1} 轮：${turns[highlightIndex].preview}` : undefined}
                aria-hidden={!visible}
                className={cn(
                    "absolute right-0 top-1/2 w-11 -translate-y-1/2 touch-none select-none",
                    visible && "pointer-events-auto",
                )}
                style={{ height: trackHeight + TRACK_PADDING_PX * 2, paddingTop: TRACK_PADDING_PX }}
                onPointerDown={handlePointerDown}
                onPointerMove={handlePointerMove}
                onPointerUp={() => finishDrag(true)}
                onPointerCancel={() => finishDrag(false)}
                data-aipp-slot="chat-mobile-turn-track"
            >
                <div ref={trackRef} className="relative w-full" style={{ height: trackHeight }}>
                    {sampleTickIndices(count, trackHeight, MIN_TICK_GAP_PX).map((index) => (
                        <span
                            key={turns[index].id}
                            aria-hidden
                            className="absolute right-2 h-[3px] w-3 -translate-y-1/2 rounded-full bg-foreground/35"
                            style={{ top: tickOffset(index, trackHeight, count) }}
                        />
                    ))}
                    {/* 高亮单独绘制：抽样后当前轮次不一定有自己的刻度 */}
                    {highlightIndex >= 0 ? (
                        <span
                            aria-hidden
                            className="absolute right-2 h-[3px] w-5 -translate-y-1/2 rounded-full bg-foreground shadow-sm ring-2 ring-background transition-[top]"
                            style={{ top: tickOffset(highlightIndex, trackHeight, count) }}
                        />
                    ) : null}
                </div>
            </div>

            {dragTurn && dragIndex !== null ? (
                <div
                    className="absolute right-14 w-max max-w-[min(70vw,280px)] -translate-y-1/2 rounded-md border border-border bg-popover px-3 py-2 text-popover-foreground shadow-md"
                    style={{
                        top: (containerHeight - trackHeight) / 2 + tickOffset(dragIndex, trackHeight, count),
                    }}
                    data-aipp-slot="chat-mobile-turn-bubble"
                >
                    <div className="text-xs text-muted-foreground">第 {dragIndex + 1} / {count} 轮</div>
                    <div className="line-clamp-2 break-all text-sm">{dragTurn.preview}</div>
                </div>
            ) : null}
        </div>
    );
}
