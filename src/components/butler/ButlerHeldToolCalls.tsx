import { useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

import McpToolCall from "@/components/McpToolCall";
import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { useToolReview } from "@/hooks/toolReviewStore";
import { useSettledFlag } from "@/hooks/useStablePresence";

// 所有工具调用都以 pending 创建：免审核/审核通过的调用会很快转为 executing，
// 只有持续停在 pending 的才是真正等待用户确认的调用。
export const HELD_TOOL_CALL_DELAY_MS = 1500;
const INPUT_CAPSULE_SELECTOR = '[data-aipp-slot="chat-input-area-container"]';
const DOCK_GAP_PX = 8;

interface Props {
    conversationId: number;
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
    /** 桌面端输入框是悬浮胶囊（绝对定位），面板需要停靠在其上方而不是参与文档流。 */
    floating?: boolean;
}

/**
 * 总管家简洁视图会隐藏工具调用卡片。这里把「自动审核不通过 / 审核失败 / 需要人工确认」
 * 的调用单独展示出来，用户执行或拒绝后状态离开 pending，卡片随之消失。
 */
export default function ButlerHeldToolCalls({ conversationId, mcpToolCallStates, floating = false }: Props) {
    const pending = useMemo(
        () => [...mcpToolCallStates.values()]
            .filter((call) => call.status === "pending" && call.conversation_id === conversationId)
            .sort((a, b) => a.call_id - b.call_id),
        [mcpToolCallStates, conversationId],
    );
    if (!pending.length) return null;
    const cards = pending.map((call) => (
        <HeldToolCall key={call.call_id} call={call} conversationId={conversationId}
            mcpToolCallStates={mcpToolCallStates} />
    ));
    if (!floating) {
        return (
            <div data-aipp-slot="butler-held-tool-calls"
                className="flex max-h-[45vh] shrink-0 flex-col gap-2 overflow-y-auto px-6 py-2 empty:hidden">
                {cards}
            </div>
        );
    }
    return <FloatingDock>{cards}</FloatingDock>;
}

/** 零高度锚点放在输入区之前，按输入胶囊的实际位置把面板停靠在它正上方并对齐宽度。 */
function FloatingDock({ children }: { children: ReactNode }) {
    const anchorRef = useRef<HTMLDivElement>(null);
    const [style, setStyle] = useState<CSSProperties>({ left: 24, right: 24, bottom: 100 });
    useLayoutEffect(() => {
        const anchor = anchorRef.current;
        const column = anchor?.parentElement;
        if (!anchor || !column) return;
        const measure = () => {
            const capsule = column.querySelector<HTMLElement>(INPUT_CAPSULE_SELECTOR);
            if (!capsule) return;
            const a = anchor.getBoundingClientRect();
            const c = capsule.getBoundingClientRect();
            const next = {
                left: Math.max(0, c.left - a.left),
                right: Math.max(0, a.right - c.right),
                bottom: a.top - c.top + DOCK_GAP_PX,
            };
            setStyle((prev) => (prev.left === next.left && prev.right === next.right && prev.bottom === next.bottom ? prev : next));
        };
        measure();
        const observer = new ResizeObserver(measure);
        observer.observe(column);
        const capsule = column.querySelector<HTMLElement>(INPUT_CAPSULE_SELECTOR);
        if (capsule) observer.observe(capsule);
        window.addEventListener("resize", measure);
        return () => { observer.disconnect(); window.removeEventListener("resize", measure); };
    }, []);
    return (
        <div ref={anchorRef} className="relative h-0 shrink-0">
            <div data-aipp-slot="butler-held-tool-calls" style={style}
                className="absolute z-20 flex max-h-[45vh] flex-col gap-2 overflow-y-auto rounded-xl border border-border bg-background p-2 shadow-lg empty:hidden">
                {children}
            </div>
        </div>
    );
}

function HeldToolCall({ call, conversationId, mcpToolCallStates }: {
    call: MCPToolCallUpdateEvent;
    conversationId: number;
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
}) {
    const review = useToolReview(call.call_id);
    // 审核进行中或已判定安全时由后端自动处理，不打扰用户；进度行会显示「处理中」。
    const handledAutomatically = review?.phase === "reviewing"
        || (review?.phase === "done" && review.verdict === "safe");
    const visible = useSettledFlag(!handledAutomatically, HELD_TOOL_CALL_DELAY_MS);
    if (!visible) return null;
    return (
        <McpToolCall
            callId={call.call_id}
            conversationId={conversationId}
            messageId={call.message_id}
            serverName={call.server_name}
            toolName={call.tool_name}
            parameters={call.parameters}
            status="pending"
            mcpToolCallStates={mcpToolCallStates}
        />
    );
}
