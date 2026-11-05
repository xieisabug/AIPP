import type { ConversationRuntimeState } from "@/data/Conversation";
import type { AvatarState } from "./avatarDefinition";

export interface ContactActivity {
    assistant_id: number;
    conversation_id: number;
    status: string | null;
    waiting_peer: boolean;
    runtime: ConversationRuntimeState;
    permission: boolean;
    question: boolean;
}
export interface ContactVisual { state: AvatarState; label: string }
export interface ContactFeedback { state: "finished" | "failed" | "notifying"; until: number }
export const successful = (status: string | null) => status === "completed" || status === "succeeded";
export const terminal = (status: string | null) => successful(status) || ["failed", "cancelled", "interrupted"].includes(status ?? "");

export function contactVisual(rows: ContactActivity[], feedback: ContactFeedback | undefined, failed: boolean, now: number): ContactVisual {
    if (rows.some(r => r.permission)) return { state: "alerting", label: "待审批" };
    if (rows.some(r => r.question)) return { state: "waiting", label: "等你回答" };
    if (failed) return { state: feedback?.state === "failed" && feedback.until > now ? "failed" : "idle", label: "执行出错" };
    const active = rows.filter(r => r.runtime.is_running);
    if (active.some(r => r.runtime.phase === "mcp_executing")) return { state: "working", label: "执行中" };
    if (active.some(r => r.runtime.phase === "assistant_streaming")) return { state: "streaming", label: "回复中" };
    if (rows.some(r => r.waiting_peer)) return { state: "waiting", label: "等待协作" };
    if (active.length) return { state: "thinking", label: "思考中" };
    if (rows.some(r => r.status === "running" || r.status === "accepted")) return { state: "working", label: "任务处理中" };
    if (feedback && feedback.until > now) return { state: feedback.state, label: feedback.state === "finished" ? "已完成" : feedback.state === "failed" ? "执行出错" : "新消息" };
    return { state: "idle", label: "" };
}
