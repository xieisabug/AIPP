import {
    useCallback,
    useMemo,
    useState,
    useLayoutEffect,
    useRef,
    type RefObject,
} from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/utils/utils";

export interface TurnRailItem {
    id: number;
    preview: string;
}

export interface ConversationTurnRailProps {
    turns: TurnRailItem[];
    scrollContainerRef: RefObject<HTMLDivElement | null>;
    onSelect: (messageId: number) => void;
}

// 自动收缩间距常量
const BAR_H = 6;
const GAP_MAX = 10;
const GAP_MIN = 2;
const HEIGHT_RATIO = 0.7;
const ACTIVE_LINE_OFFSET_PX = 56;

export function findActiveTurnId(
    turns: Pick<TurnRailItem, "id">[],
    turnPositions: Map<number, number>,
    scrollTop: number,
): number | null {
    let activeTurnId: number | null = null;
    const probeY = scrollTop + ACTIVE_LINE_OFFSET_PX;

    for (const turn of turns) {
        const top = turnPositions.get(turn.id);
        if (typeof top !== "number") {
            continue;
        }
        if (top <= probeY) {
            activeTurnId = turn.id;
            continue;
        }
        break;
    }

    return activeTurnId;
}

/**
 * 左侧对话轮次导航条。
 *
 * 渲染一列垂直居中的小横杠，每根横杠代表一次用户提问；
 * 点击触发 onSelect 跳转到该消息；悬浮显示该轮内容片段。
 * 横杠数量很多时会自动收缩间距以始终全部可见。
 */
export default function ConversationTurnRail({
    turns,
    scrollContainerRef,
    onSelect,
}: ConversationTurnRailProps) {
    const [containerHeight, setContainerHeight] = useState(0);
    const [activeTurnId, setActiveTurnId] = useState<number | null>(null);
    const syncFrameRef = useRef<number | null>(null);

    useLayoutEffect(() => {
        const el = scrollContainerRef.current;
        if (!el) {
            return;
        }
        setContainerHeight(el.clientHeight);
        const observer = new ResizeObserver((entries) => {
            const entry = entries[0];
            if (entry) {
                setContainerHeight(entry.contentRect.height);
            }
        });
        observer.observe(el);
        return () => observer.disconnect();
    }, [scrollContainerRef]);

    useLayoutEffect(() => {
        const container = scrollContainerRef.current;
        if (!container || turns.length === 0) {
            setActiveTurnId(null);
            return;
        }

        const getTurnPositions = () => {
            const containerRect = container.getBoundingClientRect();
            const positions = new Map<number, number>();

            turns.forEach((turn) => {
                const target = container.querySelector(
                    `[data-message-id='${turn.id}']`,
                ) as HTMLElement | null;
                if (!target) {
                    return;
                }

                const targetRect = target.getBoundingClientRect();
                positions.set(
                    turn.id,
                    targetRect.top - containerRect.top + container.scrollTop,
                );
            });

            return positions;
        };

        const syncActiveTurn = () => {
            syncFrameRef.current = null;
            const positions = getTurnPositions();
            const nextActiveTurnId = findActiveTurnId(
                turns,
                positions,
                container.scrollTop,
            );
            const distanceToBottom = Math.max(
                0,
                container.scrollHeight - container.scrollTop - container.clientHeight,
            );

            setActiveTurnId((current) => {
                if (nextActiveTurnId !== null) {
                    return nextActiveTurnId;
                }
                if (distanceToBottom <= 4) {
                    return turns[turns.length - 1]?.id ?? null;
                }
                if (container.scrollTop <= 4) {
                    return turns[0]?.id ?? null;
                }
                if (current !== null && turns.some((turn) => turn.id === current)) {
                    return current;
                }
                return current;
            });
        };

        const scheduleSyncActiveTurn = () => {
            if (syncFrameRef.current !== null) {
                return;
            }
            syncFrameRef.current = requestAnimationFrame(syncActiveTurn);
        };

        syncActiveTurn();
        container.addEventListener("scroll", scheduleSyncActiveTurn, {
            passive: true,
        });
        const resizeObserver = new ResizeObserver(scheduleSyncActiveTurn);
        resizeObserver.observe(container);
        const mutationObserver = new MutationObserver(scheduleSyncActiveTurn);
        mutationObserver.observe(container, {
            childList: true,
            subtree: true,
        });

        return () => {
            if (syncFrameRef.current !== null) {
                cancelAnimationFrame(syncFrameRef.current);
                syncFrameRef.current = null;
            }
            container.removeEventListener("scroll", scheduleSyncActiveTurn);
            resizeObserver.disconnect();
            mutationObserver.disconnect();
        };
    }, [scrollContainerRef, turns]);

    const gap = useMemo(() => {
        const count = turns.length;
        if (count <= 1) {
            return GAP_MAX;
        }
        const available = containerHeight * HEIGHT_RATIO;
        if (available <= 0) {
            return GAP_MAX;
        }
        const needed = count * BAR_H + (count - 1) * GAP_MAX;
        if (needed <= available) {
            return GAP_MAX;
        }
        const computed = (available - count * BAR_H) / (count - 1);
        return Math.max(GAP_MIN, computed);
    }, [turns.length, containerHeight]);

    const handleClick = useCallback(
        (id: number) => {
            setActiveTurnId(id);
            onSelect(id);
        },
        [onSelect],
    );

    if (turns.length === 0) {
        return null;
    }

    return (
        <div
            aria-hidden={false}
            className="pointer-events-none absolute left-1.5 top-1/2 z-10 flex -translate-y-1/2 flex-col items-center"
            style={{ gap: `${gap}px` }}
        >
            {turns.map((turn, i) => (
                <Tooltip key={turn.id}>
                    <TooltipTrigger asChild>
                        <button
                            type="button"
                            onClick={() => handleClick(turn.id)}
                            aria-label={`跳转到第 ${i + 1} 轮对话`}
                            aria-current={
                                activeTurnId === turn.id ? "true" : undefined
                            }
                            className={cn(
                                "pointer-events-auto h-[6px] w-7 cursor-pointer rounded-full transition-all",
                                "bg-foreground/30 hover:w-8 hover:bg-foreground",
                                activeTurnId === turn.id
                                    && "h-2 w-8 bg-foreground shadow-sm ring-2 ring-foreground/15",
                            )}
                        />
                    </TooltipTrigger>
                    <TooltipContent side="right" className="max-w-[260px]">
                        <span className="line-clamp-2 break-all text-xs">
                            {turn.preview}
                        </span>
                    </TooltipContent>
                </Tooltip>
            ))}
        </div>
    );
}
