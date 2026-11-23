/**
 * The send motion (bubble entry + slide) is driven per frame on the main thread.
 * The confirmation of an optimistic bubble re-renders the whole conversation in one
 * non-sliceable React task, which drops frames if it lands inside that window.
 * Work that does not change what is on screen can wait here until the motion ends.
 */
export const SEND_MOTION_MAX_HOLD_MS = 1000;

const holds = new Set<symbol>();
const waiting: Array<() => void> = [];
let flushScheduled = false;

function flush() {
    flushScheduled = false;
    // Tasks queued during the flush run in the same pass, after the earlier ones.
    while (holds.size === 0 && waiting.length > 0) {
        const task = waiting.shift()!;
        try {
            task();
        } catch (error) {
            console.error("[sendMotion] deferred task failed", error);
        }
    }
}

function scheduleFlush() {
    if (flushScheduled) return;
    flushScheduled = true;
    // Releases can happen inside a React commit; run the deferred work after it.
    queueMicrotask(flush);
}

/** Marks a send motion as running until the returned release is called (or the cap expires). */
export function holdForSendMotion(maxMs = SEND_MOTION_MAX_HOLD_MS): () => void {
    const token = Symbol("send-motion");
    holds.add(token);
    const release = () => {
        clearTimeout(timer);
        if (!holds.delete(token)) return;
        if (holds.size === 0) scheduleFlush();
    };
    const timer = setTimeout(release, maxMs);
    return release;
}

/**
 * Runs the task now if no send motion is running, otherwise after it ends.
 * Order is preserved: a task never overtakes one that is still waiting.
 */
export function afterSendMotion(task: () => void): void {
    if (holds.size === 0 && waiting.length === 0) {
        task();
        return;
    }
    waiting.push(task);
    if (holds.size === 0) scheduleFlush();
}

export function isSendMotionRunning(): boolean {
    return holds.size > 0;
}
