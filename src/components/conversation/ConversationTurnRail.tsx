import {
    useCallback,
    useMemo,
    useState,
    useLayoutEffect,
    type RefObject,
} from "react";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

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
                            className="pointer-events-auto h-[6px] w-7 cursor-pointer rounded-full bg-foreground/30 transition-all hover:w-8 hover:bg-foreground"
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
