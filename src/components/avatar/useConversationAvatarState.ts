import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConversationRuntimeState } from "@/data/Conversation";
import type { AvatarState } from "./avatarDefinition";

export function runtimeAvatarState(state: ConversationRuntimeState): AvatarState {
    if (!state.is_running) return "idle";
    return state.phase === "assistant_streaming" ? "responding" : state.phase === "mcp_executing" ? "working" : "thinking";
}

interface AvatarConversationEvent {
    type: string;
    data: { state?: ConversationRuntimeState; message_type?: string; has_response?: boolean; has_reasoning?: boolean };
}

export function useConversationAvatarState(conversationId: string, waiting: boolean): AvatarState {
    const [result, setResult] = useState<{ id: string; state: AvatarState }>({ id: conversationId, state: "idle" });
    useEffect(() => {
        const id = Number(conversationId);
        if (!Number.isSafeInteger(id) || id <= 0) return;
        let disposed = false;
        let revision: ConversationRuntimeState | undefined;
        let hadEvent = false;
        let terminal: "error" | "cancelled" | "success" | null = null;
        let timer: ReturnType<typeof setTimeout> | undefined;
        const update = (state: AvatarState) => { if (!disposed) setResult({ id: conversationId, state }); };
        const apply = (snapshot: ConversationRuntimeState) => {
            if (snapshot.conversation_id !== id) return;
            if (revision && (snapshot.epoch < revision.epoch || (snapshot.epoch === revision.epoch && snapshot.revision <= revision.revision))) return;
            const newRun = snapshot.is_running && (!revision || !revision.is_running || snapshot.epoch > revision.epoch);
            revision = snapshot;
            if (newRun) { terminal = null; clearTimeout(timer); }
            if (terminal) return;
            update(runtimeAvatarState(snapshot));
        };
        const listener = listen<AvatarConversationEvent>(`conversation_event_${id}`, ({ payload }) => {
            if (disposed) return;
            hadEvent = true;
            if (payload.type === "runtime_state_snapshot" && payload.data.state) apply(payload.data.state);
            else if (["message_update", "message_add"].includes(payload.type) && payload.data.message_type === "error") {
                terminal = "error"; clearTimeout(timer); update("error");
            } else if (payload.type === "conversation_cancel") {
                terminal = "cancelled"; clearTimeout(timer); update("idle");
            } else if (payload.type === "stream_complete" && !terminal && (payload.data.has_response || payload.data.has_reasoning)) {
                terminal = "success"; update("success");
                timer = setTimeout(() => { terminal = null; update(revision ? runtimeAvatarState(revision) : "idle"); }, 1000);
            }
        });
        void listener.then(async () => {
            if (disposed) return;
            const snapshot = await invoke<ConversationRuntimeState>("get_conversation_runtime_state", { conversationId: id });
            if (!disposed && !hadEvent) apply(snapshot);
        }).catch(error => console.warn(`[Avatar conversation ${id}] runtime state subscription/snapshot failed:`, error));
        return () => {
            disposed = true; clearTimeout(timer);
            void listener.then(unlisten => unlisten()).catch(error => console.warn(`[Avatar conversation ${id}] listener cleanup failed:`, error));
        };
    }, [conversationId]);
    return waiting ? "waiting" : result.id === conversationId ? result.state : "idle";
}
