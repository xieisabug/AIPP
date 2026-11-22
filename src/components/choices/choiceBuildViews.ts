import { invoke } from "@tauri-apps/api/core";
import type { PreviewRef } from "./ChoicePreview";

export type BuildView = { url: string; lease: string };
type Opening = Promise<BuildView | null>;

// Moving a card into or out of the fullscreen portal remounts its React subtree.
// Transfer the outgoing lease during that render before releasing the final view.
const outgoing = new Map<string, { ready: Opening; timer: ReturnType<typeof setTimeout> }>();

export function acquireChoiceBuild(ref: PreviewRef, rebuilt = false) {
    const key = JSON.stringify([ref.conversationId, ref.requestId, ref.candidateId, ref.collectionId]);
    const previous = rebuilt ? undefined : outgoing.get(key);
    if (previous) {
        clearTimeout(previous.timer);
        outgoing.delete(key);
    }
    const ready = previous?.ready ?? invoke<BuildView | null>(ref.collectionId !== undefined ? "open_collected_choice_build" : "open_choice_build", ref);
    function release() {
        const timer = setTimeout(() => {
            if (outgoing.get(key)?.timer === timer) outgoing.delete(key);
            void ready.then(view => view && invoke("close_choice_build", { lease: view.lease })).catch(console.error);
        }, 0);
        outgoing.set(key, { ready, timer });
    }
    return { ready, release };
}
