import {
    useCallback,
    useEffect,
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
const NODE_REFRESH_INTERVAL_MS = 200;
const CONTENT_RESIZE_SYNC_DELAY_MS = 150;

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

    const turnsRef = useRef(turns);
    turnsRef.current = turns;
    const turnsKey = useMemo(() => turns.map((turn) => turn.id).join(","), [turns]);

    useEffect(() => {
        const container = scrollContainerRef.current;
        if (!container || turnsRef.current.length === 0) {
            setActiveTurnId(null);
            return;
        }

        // 虚拟列表会卸载屏外行，已卸载轮次沿用最后一次测得的位置。
        const positions = new Map<number, number>();
        let turnNodes: HTMLElement[] = [];
        let nodesCollectedAt = -Infinity;
        let nodesDirty = true;

        const collectTurnNodes = () => {
            const turnIds = new Set(turnsRef.current.map((turn) => String(turn.id)));
            turnNodes = Array.from(
                container.querySelectorAll<HTMLElement>("[data-message-id]"),
            ).filter((node) => turnIds.has(node.dataset.messageId ?? ""));
            nodesCollectedAt = performance.now();
            nodesDirty = false;
        };

        const measureTurnPositions = () => {
            if (
                nodesDirty
                || performance.now() - nodesCollectedAt > NODE_REFRESH_INTERVAL_MS
                || turnNodes.some((node) => !node.isConnected)
            ) {
                collectTurnNodes();
            }
            const containerTop = container.getBoundingClientRect().top;
            const scrollTop = container.scrollTop;
            for (const node of turnNodes) {
                const id = Number(node.dataset.messageId);
                if (Number.isFinite(id)) {
                    positions.set(id, node.getBoundingClientRect().top - containerTop + scrollTop);
                }
            }
        };

        const publishActiveTurn = () => {
            syncFrameRef.current = null;
            measureTurnPositions();
            const nextActiveTurnId = findActiveTurnId(
                turnsRef.current,
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
                    return turnsRef.current[turnsRef.current.length - 1]?.id ?? null;
                }
                if (container.scrollTop <= 4) {
                    return turnsRef.current[0]?.id ?? null;
                }
                if (current !== null && turnsRef.current.some((turn) => turn.id === current)) {
                    return current;
                }
                return current;
            });
        };

        const scheduleSyncActiveTurn = () => {
            if (syncFrameRef.current !== null) {
                return;
            }
            syncFrameRef.current = requestAnimationFrame(publishActiveTurn);
        };

        // 内容高度变化（图片加载、展开推理、流式增长）会移动后续轮次，节流后重新测量。
        let contentResizeTimer: number | null = null;
        const handleContentResize = () => {
            nodesDirty = true;
            if (contentResizeTimer !== null) {
                return;
            }
            contentResizeTimer = window.setTimeout(() => {
                contentResizeTimer = null;
                scheduleSyncActiveTurn();
            }, CONTENT_RESIZE_SYNC_DELAY_MS);
        };

        container.addEventListener("scroll", scheduleSyncActiveTurn, {
            passive: true,
        });
        const resizeObserver = new ResizeObserver(scheduleSyncActiveTurn);
        resizeObserver.observe(container);
        const contentResizeObserver = new ResizeObserver(handleContentResize);
        Array.from(container.children).forEach((child) => contentResizeObserver.observe(child));
        scheduleSyncActiveTurn();

        return () => {
            if (syncFrameRef.current !== null) {
                cancelAnimationFrame(syncFrameRef.current);
                syncFrameRef.current = null;
            }
            if (contentResizeTimer !== null) {
                window.clearTimeout(contentResizeTimer);
            }
            container.removeEventListener("scroll", scheduleSyncActiveTurn);
            resizeObserver.disconnect();
            contentResizeObserver.disconnect();
        };
    }, [scrollContainerRef, turnsKey]);

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
            className="pointer-events-none absolute left-1.5 top-1/2 z-10 flex -translate-y-1/2 flex-col items-center w-8"
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
                                "pointer-events-auto h-[6px] w-6 cursor-pointer rounded-full transition-all",
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
