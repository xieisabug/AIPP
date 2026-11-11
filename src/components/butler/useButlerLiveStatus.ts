import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConversationRuntimePhase, ConversationRuntimeState } from "@/data/Conversation";
import { contactVisual, type ContactActivity, type ContactDetail } from "@/components/avatar/contactActivity";

const streamKinds: Record<string, ContactDetail["stream"]> = { reasoning: "reasoning", response: "response", agent_plan: "plan" };

const filePathOf = (parameters?: string) => {
    try {
        const value: unknown = JSON.parse(parameters ?? "");
        if (!value || typeof value !== "object") return undefined;
        const path = (value as { file_path?: unknown; path?: unknown }).file_path ?? (value as { path?: unknown }).path;
        return typeof path === "string" ? path : undefined;
    } catch {
        return undefined;
    }
};

const isRuntime = (value: Partial<ConversationRuntimeState> | null | undefined, conversationId: number): value is ConversationRuntimeState =>
    !!value && value.conversation_id === conversationId && typeof value.phase === "string"
    && typeof value.epoch === "number" && typeof value.revision === "number";

/**
 * 当前会话这一轮的短状态，和联系人列表用同一套 contactVisual。
 * 还没收到推理或回复 token 时是「投递中」。运行态短暂回落时保留上一句，避免和联系人列表错开。
 */
export function useButlerLiveStatus(conversationId: number): string {
    const [label, setLabel] = useState("投递中");
    useEffect(() => {
        if (!conversationId) return;
        setLabel("投递中");
        let disposed = false;
        let runtime: ConversationRuntimeState | undefined;
        let stream: ContactDetail["stream"];
        const calls = new Map<number, { tool?: string; path?: string }>();
        const executing = new Set<number>();
        const reviewing = new Set<number>();
        const publish = () => {
            if (disposed) return;
            const phase: ConversationRuntimePhase = runtime?.phase ?? "user_pending";
            const running = runtime ? runtime.is_running : true;
            const current = executing.size ? Math.max(...executing) : undefined;
            const tool = current === undefined ? undefined : calls.get(current);
            const row: ContactActivity = {
                assistant_id: 0, conversation_id: conversationId, status: null, waiting_peer: false,
                permission: false, question: false,
                runtime: { conversation_id: conversationId, is_running: running, phase, epoch: runtime?.epoch ?? 0, revision: runtime?.revision ?? 0 },
                detail: { tool: tool?.tool, path: tool?.path, reviewing: reviewing.size > 0, stream },
            };
            const next = contactVisual([row], undefined, false, 0).label;
            if (next) setLabel(next);
        };
        const apply = (next: ConversationRuntimeState) => {
            if (runtime && (next.epoch < runtime.epoch || (next.epoch === runtime.epoch && next.revision <= runtime.revision))) return;
            // 只在进入新一轮时清掉上一轮的工具和 token 类型；第一次快照不能清掉已经到达的事件。
            if (runtime && next.epoch > runtime.epoch) {
                stream = undefined; calls.clear(); executing.clear(); reviewing.clear();
            }
            if (!next.is_running) { executing.clear(); reviewing.clear(); stream = undefined; }
            runtime = next;
        };
        type Event = { type: string; data: {
            state?: ConversationRuntimeState; message_type?: string;
            call_id?: number; status?: string; tool_name?: string; parameters?: string; phase?: string;
        } };
        const unlisten = listen<Event>(`conversation_event_${conversationId}`, ({ payload: event }) => {
            if (disposed) return;
            if (event.type === "runtime_state_snapshot" && event.data.state) apply(event.data.state);
            else if (event.type === "mcp_tool_call_update" && typeof event.data.call_id === "number") {
                const callId = event.data.call_id;
                const old = calls.get(callId) ?? {};
                const parameters = event.data.parameters;
                calls.set(callId, {
                    tool: event.data.tool_name ?? old.tool,
                    path: (parameters ? filePathOf(parameters) : undefined) ?? old.path,
                });
                if (event.data.status === "executing") executing.add(callId);
                else if (event.data.status !== "pending") executing.delete(callId);
            } else if (event.type === "tool_review_update" && typeof event.data.call_id === "number") {
                if (event.data.phase === "reviewing") reviewing.add(event.data.call_id);
                else reviewing.delete(event.data.call_id);
            } else if (["message_add", "message_update"].includes(event.type) && event.data.message_type && streamKinds[event.data.message_type]) {
                stream = streamKinds[event.data.message_type];
            }
            publish();
        });
        void invoke<ConversationRuntimeState>("get_conversation_runtime_state", { conversationId })
            .then(snapshot => { if (!disposed && isRuntime(snapshot, conversationId)) { apply(snapshot); publish(); } })
            .catch(error => console.warn(`[Butler live status] snapshot failed (${conversationId}):`, error));
        return () => {
            disposed = true;
            void unlisten.then(stop => stop()).catch(() => {});
        };
    }, [conversationId]);
    return label;
}
