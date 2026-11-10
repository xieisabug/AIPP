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
import { visibleText } from "@/utils/turnContext";

export interface TurnRailItem {
    id: number;
    preview: string;
}

// 从用户消息内容生成悬浮预览片段（私聊与群聊共用）；只取用户原话，不含本轮附加上下文
const TURN_PREVIEW_MAX = 120;
export function makeTurnPreview(content: string): string {
    const collapsed = visibleText(content).replace(/\s+/g, " ").trim();
    if (collapsed.length <= TURN_PREVIEW_MAX) {
        return collapsed;
    }
    return collapsed.slice(0, TURN_PREVIEW_MAX) + "…";
}

export interface ConversationTurnRailProps {
    turns: TurnRailItem[];
    scrollContainerRef: RefObject<HTMLDivElement | null>;
    onSelect: (messageId: number) => void;
}

// 自动收缩间距常量：BAR_H 为每个刻度的点击热区高度（可见细线仅 3px，与移动端滑轨一致）
const BAR_H = 10;
const GAP_MAX = 6;
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
 * 跟踪滚动容器高度与当前阅读位置所在的轮次，供桌面导航条与移动端轮次滑轨共用。
 */
export function useTurnRailTracking(
    turns: TurnRailItem[],
    scrollContainerRef: RefObject<HTMLDivElement | null>,
) {
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

    return { containerHeight, activeTurnId, setActiveTurnId };
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
    const { containerHeight, activeTurnId, setActiveTurnId } = useTurnRailTracking(turns, scrollContainerRef);

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
            className="pointer-events-none absolute left-1.5 top-1/2 z-10 flex -translate-y-1/2 flex-col items-start w-8"
            style={{ gap: `${gap}px` }}
        >
            {turns.map((turn, i) => {
                const isActive = activeTurnId === turn.id;
                return (
                <Tooltip key={turn.id}>
                    <TooltipTrigger asChild>
                        <button
                            type="button"
                            onClick={() => handleClick(turn.id)}
                            aria-label={`跳转到第 ${i + 1} 轮对话`}
                            aria-current={isActive ? "true" : undefined}
                            className="group pointer-events-auto flex h-2.5 w-8 shrink-0 cursor-pointer items-center pl-1"
                        >
                            <span
                                aria-hidden
                                className={cn(
                                    "block h-[3px] rounded-full transition-all duration-150",
                                    isActive
                                        ? "w-5 bg-foreground shadow-sm ring-2 ring-background"
                                        : "w-3 bg-foreground/35 group-hover:w-5 group-hover:bg-foreground/70",
                                )}
                            />
                        </button>
                    </TooltipTrigger>
                    <TooltipContent side="right" className="max-w-[260px]">
                        <span className="line-clamp-2 break-all text-xs">
                            {turn.preview}
                        </span>
                    </TooltipContent>
                </Tooltip>
                );
            })}
        </div>
    );
}
