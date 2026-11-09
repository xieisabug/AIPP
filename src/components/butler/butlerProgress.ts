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
        if (!conversationId) return;
        let disposed = false;
        const accept = (next: ButlerProgressSummary | null | undefined) => {
            if (disposed || !next || next.conversation_id !== conversationId) return;
            setSummary(previous => previous && previous.epoch > next.epoch ? previous : next);
        };
        const unlisten = listen<ButlerProgressSummary>(BUTLER_PROGRESS_EVENT, ({ payload }) => accept(payload));
        invoke<ButlerProgressSummary | null>("get_butler_progress_summary", { conversationId })
            .then(accept)
            .catch(error => console.warn(`[Butler progress] snapshot failed (${conversationId}):`, error));
        return () => {
            disposed = true;
            void unlisten.then(stop => stop()).catch(() => {});
        };
    }, [conversationId]);
    return summary;
}
