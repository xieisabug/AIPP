import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ButlerReport } from "@/data/Conversation";

// Kept in sync with src-tauri/src/api/butler_progress.rs.
export const BUTLER_PROGRESS_EVENT = "butler_progress_summary";
export interface ButlerProgressStep { label: string; status: string }
export interface ButlerProgressSummary { source: "report_progress" | "helper_summary"; conversation_id: number; epoch: number; text: string; steps: ButlerProgressStep[] }

export interface ProgressSnapshot extends ButlerProgressSummary { reports: ButlerReport[]; helper: ButlerProgressSummary | null }

/** 当前会话这一轮的进度摘要；只接受不早于已知轮次的结果，避免上一轮遗留的摘要回写。 */
export function useButlerProgress(conversationId: number): ProgressSnapshot | undefined {
    const [summary, setSummary] = useState<ProgressSnapshot>();
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
                setSummary(previous => state.is_running && previous?.epoch === state.epoch ? previous : undefined);
            },
        );
        const accept = (next: ProgressSnapshot | null | undefined) => {
            if (disposed || !next || next.conversation_id !== conversationId || next.epoch < epoch) return;
            if (next.epoch === epoch && !running) return;
            if (next.epoch > epoch) {
                epoch = next.epoch;
                revision = -1;
                running = true;
            }
            setSummary(previous => previous?.conversation_id === conversationId && previous.epoch > next.epoch ? previous : next);
        };
        let latestEventEpoch = -1;
        const unlisten = listen<ProgressSnapshot>(BUTLER_PROGRESS_EVENT, ({ payload }) => {
            if (payload.conversation_id !== conversationId) return;
            latestEventEpoch = Math.max(latestEventEpoch, payload.epoch);
            accept(payload);
        });
        void unlisten.then(() => invoke<ProgressSnapshot | null>("get_butler_progress_summary", { conversationId }))
            .then(snapshot => { if (snapshot && snapshot.epoch > latestEventEpoch) accept(snapshot); })
            .catch(error => console.warn(`[Butler progress] snapshot failed (${conversationId}):`, error));
        return () => {
            disposed = true;
            void runtimeListener.then(stop => stop()).catch(() => {});
            void unlisten.then(stop => stop()).catch(() => {});
        };
    }, [conversationId]);
    return summary?.conversation_id === conversationId ? summary : undefined;
}
