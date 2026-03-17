type ShiningMessageSetter =
    | Set<number>
    | ((prev: Set<number>) => Set<number>);

interface ScrollHighlightOptions {
    target: HTMLElement;
    messageId: number;
    setShiningMessageIds: (value: ShiningMessageSetter) => void;
    clearPendingScrollMessageId: (messageId: number | null) => void;
    requestFrame?: (callback: FrameRequestCallback) => number;
    scheduleTimeout?: (callback: () => void, delayMs: number) => ReturnType<typeof setTimeout>;
}

export function applyScrollHighlight({
    target,
    messageId,
    setShiningMessageIds,
    clearPendingScrollMessageId,
    requestFrame = requestAnimationFrame,
    scheduleTimeout = (callback, delayMs) => setTimeout(callback, delayMs),
}: ScrollHighlightOptions) {
    requestFrame(() => {
        target.scrollIntoView({ behavior: "smooth", block: "center" });
        setShiningMessageIds(() => new Set([messageId]));
        scheduleTimeout(() => {
            setShiningMessageIds(new Set());
        }, 2000);
        clearPendingScrollMessageId(null);
    });
}
