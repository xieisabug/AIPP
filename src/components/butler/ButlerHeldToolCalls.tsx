import { ReviewModelHint } from "../mcp-tool-components/ReviewModelHint";
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Check, ChevronRight, Loader2, ShieldAlert, X } from "lucide-react";
import { toast } from "sonner";

import { Button } from "@/components/ui/button";
import { JsonDisplay } from "@/components/McpToolCall";
import { useAntiLeakage } from "@/contexts/AntiLeakageContext";
import type { MCPToolCallUpdateEvent } from "@/data/Conversation";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { useToolReview, type ToolReviewView } from "@/hooks/toolReviewStore";
import { useSettledFlag } from "@/hooks/useStablePresence";
import { maskToolCall } from "@/utils/antiLeakage";
import { getErrorMessage } from "@/utils/error";

// 所有工具调用都以 pending 创建：免审核/审核通过的调用会很快转为 executing，
// 只有持续停在 pending、且没有自动审核在进行的，才是真正等待用户确认的调用。
export const HELD_TOOL_CALL_DELAY_MS = 1500;
const INPUT_CAPSULE_SELECTOR = '[data-aipp-slot="chat-input-area-container"]';
const DOCK_GAP_PX = 8;
const SUMMARY_VALUE_MAX = 40;

interface Props {
    conversationId: number;
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent>;
    /** 桌面端输入框是悬浮胶囊（绝对定位），面板需要停靠在其上方而不是参与文档流。 */
    floating?: boolean;
}

/**
 * 总管家简洁视图会隐藏工具调用卡片。等待用户确认的调用单独展示在输入框上方。
 * 自动审核进行中不弹出；审核通过后调用会离开 pending，卡片不出现。
 * 只有自动审核未通过（有风险 / 失败），或没有审核、需要人工确认时，才弹出待确认卡片。
 * 用户允许或拒绝后状态离开 pending，卡片随之消失。
 */
export default function ButlerHeldToolCalls({ conversationId, mcpToolCallStates, floating = false }: Props) {
    const pending = useMemo(
        () => [...mcpToolCallStates.values()]
            .filter((call) => call.status === "pending" && call.conversation_id === conversationId)
            .sort((a, b) => a.call_id - b.call_id),
        [mcpToolCallStates, conversationId],
    );
    if (!pending.length) return null;
    const cards = pending.map((call) => <HeldToolCall key={call.call_id} call={call} />);
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
                className="absolute z-20 flex max-h-[45vh] flex-col gap-2 overflow-y-auto empty:hidden">
                {children}
            </div>
        </div>
    );
}

function HeldToolCall({ call }: { call: MCPToolCallUpdateEvent }) {
    const review = useToolReview(call.call_id);
    const reviewing = review?.phase === "reviewing";
    const verdict = review?.phase === "done" ? review.verdict : null;
    // 还没有审核事件：免审核调用会很快转为 executing，稍等再判断是否真的需要人工确认。
    const unknownSettled = useSettledFlag(review === null, HELD_TOOL_CALL_DELAY_MS);

    // 自动审核进行中、审核通过、或用户已手动处理：不弹卡片。
    // 审核结论已出（有风险/失败）立即请用户确认；一直没有审核事件则等延迟后再当作人工确认。
    const visible = !reviewing
        && verdict !== "safe"
        && verdict !== "cancelled"
        && (review !== null || unknownSettled);
    if (!visible) return null;
    return <HeldToolCallCard call={call} review={review} />;
}

/** 审核结论放在首位：用户首先需要知道「为什么被拦下」，其次才是参数细节。 */
export function describeHold(review: ToolReviewView | null): { title: string; reason: string } {
    if (review?.phase === "done") {
        const reason = review.reason.trim();
        if (review.verdict === "risky") return { title: "自动审核认为有风险", reason };
        if (review.verdict === "error") return { title: "自动审核失败，需要你确认", reason };
    }
    return { title: "需要你确认后执行", reason: "" };
}

const shortValue = (value: unknown): string => {
    const text = typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
    const flat = text.replace(/\s+/g, " ");
    return flat.length > SUMMARY_VALUE_MAX ? `${flat.slice(0, SUMMARY_VALUE_MAX)}…` : flat;
};

/** 参数折叠时的一行摘要：顶层字段 `key: value`，非对象参数直接截断显示。 */
export function summarizeParameters(parameters: string): string {
    try {
        const parsed: unknown = JSON.parse(parameters);
        if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
            return Object.entries(parsed).map(([key, value]) => `${key}: ${shortValue(value)}`).join(" · ");
        }
        return shortValue(parsed);
    } catch {
        return shortValue(parameters);
    }
}

type Decision = "execute" | "reject";

function HeldToolCallCard({ call, review }: { call: MCPToolCallUpdateEvent; review: ToolReviewView | null }) {
    const isMobile = useIsMobilePlatform();
    const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
    const [expanded, setExpanded] = useState(false);
    const [deciding, setDeciding] = useState<Decision | null>(null);
    const decidingRef = useRef(false);

    const raw = {
        serverName: call.server_name ?? "未知服务器",
        toolName: call.tool_name ?? "未知工具",
        parameters: call.parameters ?? "{}",
    };
    const display = antiLeakageEnabled && !isRevealed
        ? maskToolCall(raw.serverName, raw.toolName, raw.parameters)
        : raw;
    const summary = useMemo(() => summarizeParameters(display.parameters), [display.parameters]);
    const hold = describeHold(review);

    const decide = useCallback(async (decision: Decision) => {
        if (decidingRef.current) return;
        decidingRef.current = true;
        setDeciding(decision);
        try {
            // 卡片在后端推送新状态后自然卸载；失败时卡片仍在，可直接重试。
            if (decision === "execute") {
                await invoke("execute_mcp_tool_call", { callId: call.call_id, triggerContinuation: true });
            } else {
                await invoke("reject_mcp_tool_call", { callId: call.call_id });
            }
        } catch (error) {
            const action = decision === "execute" ? "执行" : "拒绝";
            toast.error(`${action}工具 ${call.tool_name ?? "未知工具"} 失败（调用 #${call.call_id}）：${getErrorMessage(error) || "后端未返回错误原因"}`);
        } finally {
            decidingRef.current = false;
            setDeciding(null);
        }
    }, [call.call_id, call.tool_name]);

    const buttonSize = isMobile ? "min-h-11 px-4" : "h-8 px-3";
    return (
        <div data-aipp-slot="butler-held-tool-call" data-aipp-state="held" role="group"
            aria-label={`待确认的工具调用 ${display.toolName}`}
            className="rounded-xl border border-border bg-background p-3 text-sm shadow-md">
            <div className="flex items-start gap-2">
                <ShieldAlert aria-hidden="true" className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
                <div className="min-w-0 flex-1 space-y-0.5">
                    <div className="flex min-w-0 items-baseline gap-1.5">
                        <span className="truncate font-medium" title={display.toolName}>{display.toolName}</span>
                        <span className="shrink-0 truncate text-xs text-muted-foreground" title={display.serverName}>{display.serverName}</span>
                    </div>
                    <p className="text-xs text-foreground" data-aipp-slot="butler-held-tool-call-reason" aria-live="polite">
                        <span className="font-medium">{hold.title}</span>
                        {hold.reason ? <span className="text-muted-foreground">：{hold.reason}</span> : null}
                    </p>
                </div>
            </div>

            <button type="button" onClick={() => setExpanded(!expanded)} aria-expanded={expanded}
                className={`mt-2 flex w-full min-w-0 items-center gap-1 rounded-md px-1 text-left text-xs text-muted-foreground hover:bg-muted hover:text-foreground ${isMobile ? "min-h-11" : "py-1"}`}>
                <ChevronRight aria-hidden="true" className={`size-3 shrink-0 transition-transform ${expanded ? "rotate-90" : ""}`} />
                <span className="shrink-0">{expanded ? "收起参数" : "参数"}</span>
                {!expanded && summary ? <span className="min-w-0 truncate font-mono" title={summary}>{summary}</span> : null}
            </button>
            {expanded ? <JsonDisplay content={display.parameters} maxHeight="160px" className="mt-1" /> : null}

            <div className="mt-2 flex flex-wrap items-center justify-end gap-2">
                <Button size="sm" variant="outline" className={buttonSize} disabled={deciding !== null}
                    onClick={() => void decide("reject")}>
                    {deciding === "reject" ? <Loader2 className="h-4 w-4 animate-spin" /> : <X className="h-4 w-4" />}
                    拒绝
                </Button>
                <Button size="sm" variant="default" className={buttonSize} disabled={deciding !== null}
                    onClick={() => void decide("execute")}>
                    {deciding === "execute" ? <Loader2 className="h-4 w-4 animate-spin" /> : <Check className="h-4 w-4" />}
                    允许执行
                </Button>
            </div>
            {review?.phase === "done" && review.verdict === "manual" && <ReviewModelHint />}
        </div>
    );
}
