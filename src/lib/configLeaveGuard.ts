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
