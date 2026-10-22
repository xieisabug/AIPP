import { useCallback, useSyncExternalStore } from "react";

const MAX_TRACKED_GROUPS = 2000;

const widthByGroup = new Map<string, number>();
const listenersByGroup = new Map<string, Set<() => void>>();

/**
 * Records the natural width of one segment of a split merged bubble. The group
 * keeps the widest width seen so far, so every segment can share one bubble
 * width even though segments mount and unmount independently.
 */
export function reportMergedGroupSegmentWidth(groupKey: string, width: number): void {
    const rounded = Math.ceil(width);
    const previous = widthByGroup.get(groupKey) ?? 0;
    if (rounded <= previous) {
        return;
    }

    widthByGroup.delete(groupKey);
    widthByGroup.set(groupKey, rounded);
    if (widthByGroup.size > MAX_TRACKED_GROUPS) {
        const oldest = widthByGroup.keys().next().value;
        if (oldest !== undefined) {
            widthByGroup.delete(oldest);
        }
    }
    listenersByGroup.get(groupKey)?.forEach((listener) => listener());
}

export function useMergedGroupWidth(groupKey: string): number {
    const subscribe = useCallback(
        (listener: () => void) => {
            let listeners = listenersByGroup.get(groupKey);
            if (!listeners) {
                listeners = new Set();
                listenersByGroup.set(groupKey, listeners);
            }
            listeners.add(listener);
            return () => {
                listeners.delete(listener);
                if (listeners.size === 0) {
                    listenersByGroup.delete(groupKey);
                }
            };
        },
        [groupKey],
    );
    const getSnapshot = useCallback(() => widthByGroup.get(groupKey) ?? 0, [groupKey]);
    return useSyncExternalStore(subscribe, getSnapshot);
}
