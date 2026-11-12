import {
    useEffect,
    useMemo,
    useState,
    useLayoutEffect,
    useRef,
    type KeyboardEvent as ReactKeyboardEvent,
    type RefObject,
} from "react";
import { visibleText } from "@/utils/turnContext";
import { railOffsetTop, tickOffset, tickThickness, turnIndexFromOffset } from "./turnRailLayout";

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

/** 轮次少时相邻刻度的间距。 */
const SLOT_PX = 16;
/** 轮次多时整条轨道最多占滚动区高度的比例，不再随轮数变长。 */
const TRACK_HEIGHT_RATIO = 0.5;
/** 首尾刻度外的额外热区。 */
const TRACK_PADDING_PX = 6;
/** 键盘 PageUp/PageDown 一次跨越的轮数。 */
const PAGE_STEP = 10;
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
 * 跟踪滚动容器的位置与高度，以及当前阅读位置所在的轮次，供桌面导航条与移动端轮次滑轨共用。
 */
export function useTurnRailTracking(
    turns: TurnRailItem[],
    scrollContainerRef: RefObject<HTMLDivElement | null>,
) {
    const [containerHeight, setContainerHeight] = useState(0);
    const [containerTop, setContainerTop] = useState(0);
    const [activeTurnId, setActiveTurnId] = useState<number | null>(null);
    const syncFrameRef = useRef<number | null>(null);
    /** 供后续重新尝试挂载测量（ref 晚就绪时兜底）。 */
    const attachRef = useRef<() => void>(() => {});

    useLayoutEffect(() => {
        let observed: HTMLDivElement | null = null;
        let observer: ResizeObserver | null = null;
        let retryFrame = 0;
        let retries = 0;
        let disposed = false;

        // 导航条必须对齐滚动区（上方可能有标题/横幅，下方是输入区），不能按整个会话根容器居中。
        const measure = () => {
            if (!observed) {
                return;
            }
            setContainerHeight(observed.clientHeight);
            setContainerTop(observed.offsetTop);
        };

        // 同一个 commit 里导航条会先于滚动区挂载（它渲染在滚动区之前），此时 ref 还是空的，
        // 量不到尺寸就只能等到它挂上（管家窗口按会话 key 重挂时每次都会碰到）。
        const attach = () => {
            if (disposed) {
                return;
            }
            const el = scrollContainerRef.current;
            if (!el) {
                if (retries < 30) {
                    retries += 1;
                    retryFrame = requestAnimationFrame(attach);
                }
                return;
            }
            if (el !== observed) {
                observed = el;
                observer?.disconnect();
                observer = new ResizeObserver(measure);
                observer.observe(el);
            }
            measure();
        };

        attach();
        attachRef.current = attach;
        return () => {
            attachRef.current = () => {};
            disposed = true;
            cancelAnimationFrame(retryFrame);
            observer?.disconnect();
        };
    }, [scrollContainerRef]);

    const turnsRef = useRef(turns);
    turnsRef.current = turns;
    const turnsKey = useMemo(() => turns.map((turn) => turn.id).join(","), [turns]);

    useEffect(() => {
        // 轮次变化时再试一次：即使首帧的重试帧用完，只要消息/会话继续变化就能重新挂上测量。
        attachRef.current();
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

    return { containerHeight, containerTop, activeTurnId, setActiveTurnId };
}

/** 桌面导航条轨道高度：轮次少时每轮 SLOT_PX，多了就封顶在滚动区高度的 TRACK_HEIGHT_RATIO。 */
export function railTrackHeight(count: number, containerHeight: number): number {
    if (count <= 1) {
        return 0;
    }
    return Math.min((count - 1) * SLOT_PX, Math.max(0, containerHeight * TRACK_HEIGHT_RATIO));
}

/**
 * 左侧对话轮次导航条。
 *
 * 每轮用户提问对应轨道上的一个位置，当前阅读轮次用实心刻度标出。
 * 轨道高度有上限：轮次少时逐轮画刻度，轮次多时轨道不再变长，刻度自动变薄（最细 1px），
 * 每一轮都保留自己的刻度，不会漏轮。整条轨道与消息区对齐并垂直居中。
 * 整条轨道是一个滑块：悬浮时吸附到最近的一轮并显示「第 i / N 轮」与提问片段，点击跳转；
 * 获得键盘焦点后可用方向键 / PageUp / PageDown / Home / End 预览，Enter 跳转。
 */
export default function ConversationTurnRail({
    turns,
    scrollContainerRef,
    onSelect,
}: ConversationTurnRailProps) {
    const { containerHeight, containerTop, activeTurnId, setActiveTurnId } = useTurnRailTracking(turns, scrollContainerRef);
    const [previewIndex, setPreviewIndex] = useState<number | null>(null);
    const trackRef = useRef<HTMLDivElement>(null);

    const count = turns.length;
    const trackHeight = railTrackHeight(count, containerHeight);
    const thickness = tickThickness(count, trackHeight);
    // 每轮一根刻度（不抽样）；数量大时 memo，避免滚动时反复重建几百个节点。
    const ticks = useMemo(
        () =>
            turns.map((turn, index) => (
                <span
                    key={turn.id}
                    aria-hidden
                    data-turn-tick=""
                    className="absolute left-1 w-3 -translate-y-1/2 rounded-full bg-foreground/35"
                    style={{ top: tickOffset(index, trackHeight, count), height: thickness }}
                />
            )),
        [turns, count, trackHeight, thickness],
    );

    // 还没量到消息区高度时不画：此时轨道高度为 0，所有刻度会叠在顶部变成一根。
    if (count === 0 || containerHeight <= 0) {
        return null;
    }

    // 轮次列表变短（删除消息、切换会话）后旧的预览下标可能越界。
    const shownPreview = previewIndex !== null && previewIndex < count ? previewIndex : null;
    const activeIndex = turns.findIndex((turn) => turn.id === activeTurnId);
    const valueIndex = shownPreview ?? activeIndex;
    const previewTurn = shownPreview === null ? null : turns[shownPreview];

    const indexFromPointer = (clientY: number) => {
        const top = trackRef.current?.getBoundingClientRect().top ?? 0;
        return turnIndexFromOffset(clientY - top, trackHeight, count);
    };

    const select = (index: number) => {
        const turn = turns[index];
        if (!turn) {
            return;
        }
        setActiveTurnId(turn.id);
        onSelect(turn.id);
    };

    const handleKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
        const base = shownPreview ?? Math.max(activeIndex, 0);
        let next: number;
        switch (event.key) {
            case "ArrowUp":
            case "ArrowLeft":
                next = base - 1;
                break;
            case "ArrowDown":
            case "ArrowRight":
                next = base + 1;
                break;
            case "PageUp":
                next = base - PAGE_STEP;
                break;
            case "PageDown":
                next = base + PAGE_STEP;
                break;
            case "Home":
                next = 0;
                break;
            case "End":
                next = count - 1;
                break;
            case "Enter":
            case " ":
                event.preventDefault();
                setPreviewIndex(null);
                select(base);
                return;
            case "Escape":
                setPreviewIndex(null);
                return;
            default:
                return;
        }
        event.preventDefault();
        setPreviewIndex(Math.min(Math.max(next, 0), count - 1));
    };

    const markerTop = (index: number) => tickOffset(index, trackHeight, count);

    return (
        <div
            className="pointer-events-none absolute left-1 z-10"
            style={{ top: containerTop, height: containerHeight }}
            data-aipp-slot="chat-turn-rail"
        >
            <div
                role="slider"
                tabIndex={0}
                aria-label="对话轮次导航"
                aria-orientation="vertical"
                aria-valuemin={1}
                aria-valuemax={count}
                aria-valuenow={valueIndex >= 0 ? valueIndex + 1 : undefined}
                aria-valuetext={
                    valueIndex >= 0 ? `第 ${valueIndex + 1} 轮：${turns[valueIndex].preview}` : undefined
                }
                className="pointer-events-auto absolute left-0 w-6 cursor-pointer rounded-sm outline-none focus-visible:ring-1 focus-visible:ring-ring"
                style={{
                    top: railOffsetTop(trackHeight, containerHeight, TRACK_PADDING_PX),
                    height: trackHeight + TRACK_PADDING_PX * 2,
                    paddingTop: TRACK_PADDING_PX,
                }}
                // 点击导航条不抢输入框焦点；键盘用户仍可 Tab 进入。
                onMouseDown={(event) => event.preventDefault()}
                onPointerMove={(event) => {
                    const next = indexFromPointer(event.clientY);
                    setPreviewIndex((current) => (current === next ? current : next));
                }}
                onPointerLeave={() => setPreviewIndex(null)}
                onClick={(event) => select(indexFromPointer(event.clientY))}
                onKeyDown={handleKeyDown}
                onBlur={() => setPreviewIndex(null)}
                data-aipp-slot="chat-turn-rail-track"
            >
                <div ref={trackRef} className="relative" style={{ height: trackHeight }}>
                    {ticks}
                    {shownPreview !== null && shownPreview !== activeIndex ? (
                        <span
                            aria-hidden
                            className="absolute left-1 h-[3px] w-4 -translate-y-1/2 rounded-full bg-foreground/70"
                            style={{ top: markerTop(shownPreview) }}
                        />
                    ) : null}
                    {activeIndex >= 0 ? (
                        <span
                            aria-hidden
                            data-turn-active=""
                            className="absolute left-1 h-[3px] w-4 -translate-y-1/2 rounded-full bg-foreground shadow-sm ring-2 ring-background transition-[top] duration-150"
                            style={{ top: markerTop(activeIndex) }}
                        />
                    ) : null}
                </div>

                {previewTurn && shownPreview !== null ? (
                    <div
                        className="pointer-events-none absolute left-full ml-1.5 w-max max-w-[260px] -translate-y-1/2 rounded-md border border-border bg-popover px-3 py-1.5 text-xs text-popover-foreground shadow-md"
                        style={{ top: TRACK_PADDING_PX + markerTop(shownPreview) }}
                        data-aipp-slot="chat-turn-rail-preview"
                    >
                        <div className="text-muted-foreground">第 {shownPreview + 1} / {count} 轮</div>
                        {previewTurn.preview ? (
                            <div className="line-clamp-2 break-all">{previewTurn.preview}</div>
                        ) : null}
                    </div>
                ) : null}
            </div>
        </div>
    );
}
