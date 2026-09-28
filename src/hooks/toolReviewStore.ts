import { useEffect, useSyncExternalStore } from "react";
import { invoke } from "@tauri-apps/api/core";

export type ToolReviewView =
    | { phase: "reviewing"; callId: number }
    | { phase: "done"; callId: number; verdict: string; reason: string };

type Listener = () => void;

let reviews = new Map<number, ToolReviewView>();
const listeners = new Set<Listener>();

function emit() {
    listeners.forEach((listener) => listener());
}

export function noteToolReview(callId: number, next: ToolReviewView) {
    const current = reviews.get(callId);
    if (current?.phase === "done" && next.phase === "reviewing") {
        return;
    }
    if (
        current?.phase === "done"
        && next.phase === "done"
        && current.verdict === next.verdict
        && current.reason === next.reason
    ) {
        return;
    }
    const updated = new Map(reviews);
    updated.set(callId, next);
    reviews = updated;
    emit();
}

export function resetToolReviewsForTests() {
    reviews = new Map();
    emit();
}

function subscribe(listener: Listener) {
    listeners.add(listener);
    return () => listeners.delete(listener);
}

function getSnapshot() {
    return reviews;
}

export function useToolReview(callId?: number | null): ToolReviewView | null {
    const snapshot = useSyncExternalStore(subscribe, getSnapshot, getSnapshot);
    useEffect(() => {
        if (!callId || reviews.has(callId)) return;
        let cancelled = false;
        invoke<{ verdict?: string; reason?: string } | null>("get_tool_review", { callId })
            .then((loaded) => {
                // A live event is newer than this historical read, even if it is still reviewing.
                if (cancelled || reviews.has(callId) || !loaded?.verdict) return;
                noteToolReview(callId, {
                    phase: "done",
                    callId,
                    verdict: loaded.verdict,
                    reason: loaded.reason?.trim() || "",
                });
            })
            .catch(() => undefined);
        return () => { cancelled = true; };
    }, [callId]);
    if (!callId) {
        return null;
    }
    return snapshot.get(callId) ?? null;
}
