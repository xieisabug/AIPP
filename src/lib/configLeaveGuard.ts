/** Settings leave guard: AssistantConfig registers while dirty so menu/home can prompt. */

type LeaveProceed = () => void;

let beforeLeaveHandler: ((proceed: LeaveProceed) => void) | null = null;

export function registerConfigBeforeLeave(handler: (proceed: LeaveProceed) => void): () => void {
    beforeLeaveHandler = handler;
    return () => {
        if (beforeLeaveHandler === handler) beforeLeaveHandler = null;
    };
}

/** Run `proceed` immediately, or after the registered dirty-form prompt resolves. */
export function requestConfigLeave(proceed: LeaveProceed): void {
    if (beforeLeaveHandler) {
        beforeLeaveHandler(proceed);
        return;
    }
    proceed();
}

/**
 * Synchronous variant for system back: returns true when leaving may happen right now.
 * Otherwise the guard prompt is showing and `onDeferredProceed` runs once the user confirms.
 */
export function canLeaveConfigNow(onDeferredProceed: LeaveProceed): boolean {
    let immediate = false;
    let deferred = false;
    requestConfigLeave(() => {
        if (deferred) onDeferredProceed();
        else immediate = true;
    });
    deferred = true;
    return immediate;
}
