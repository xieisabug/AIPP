import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Kept in sync with src-tauri/src/api/butler_progress.rs.
export const BUTLER_PROGRESS_EVENT = "butler_progress_summary";
export interface ButlerProgressStep { label: string; status: string }
export interface ButlerProgressSummary { conversation_id: number; epoch: number; text: string; steps: ButlerProgressStep[] }

/** 当前会话这一轮的进度摘要；只接受不早于已知轮次的结果，避免上一轮遗留的摘要回写。 */
export function useButlerProgress(conversationId: number): ButlerProgressSummary | undefined {
    const [summary, setSummary] = useState<ButlerProgressSummary>();
    useEffect(() => {
        setSummary(undefined);
        if (!conversationId) return;
        let disposed = false;
        let epoch = -1;
        let revision = -1;
        let running = true;
        const runtimeListener = listen<{ type: string; data: { state?: { epoch: number; revision: number; is_running: boolean } } }>(
            `conversation_event_${conversationId}`, ({ payload }) => {
                const state = payload.data.state;
                if (disposed || payload.type !== "runtime_state_snapshot" || !state || state.epoch < epoch) return;
                if (state.epoch === epoch && state.revision < revision) return;
                epoch = state.epoch;
                revision = state.revision;
                running = state.is_running;
                setSummary(previous => state.is_running && previous?.epoch === epoch ? previous : undefined);
            },
        );
        const accept = (next: ButlerProgressSummary | null | undefined) => {
            if (disposed || !next || next.conversation_id !== conversationId || next.epoch < epoch) return;
            if (next.epoch === epoch && !running) return;
            if (next.epoch > epoch) {
                epoch = next.epoch;
                revision = -1;
                running = true;
            }
            setSummary(previous => previous?.conversation_id === conversationId && previous.epoch > next.epoch ? previous : next);
        };
        let receivedEvent = false;
        const unlisten = listen<ButlerProgressSummary>(BUTLER_PROGRESS_EVENT, ({ payload }) => {
            if (payload.conversation_id !== conversationId) return;
            receivedEvent = true;
            accept(payload);
        });
        void unlisten.then(() => invoke<ButlerProgressSummary | null>("get_butler_progress_summary", { conversationId }))
            .then(snapshot => { if (!receivedEvent) accept(snapshot); })
            .catch(error => console.warn(`[Butler progress] snapshot failed (${conversationId}):`, error));
        return () => {
            disposed = true;
            void runtimeListener.then(stop => stop()).catch(() => {});
            void unlisten.then(stop => stop()).catch(() => {});
        };
    }, [conversationId]);
    return summary?.conversation_id === conversationId ? summary : undefined;
}
