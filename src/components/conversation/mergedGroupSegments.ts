/** Merged bubbles shorter than this stay a single virtual row. */
export const MERGED_GROUP_SPLIT_MIN_HEIGHT_PX = 720;
/** A segment is closed once its visible parts reach this height. */
export const MERGED_GROUP_SEGMENT_TARGET_PX = 360;

/**
 * Splits a merged assistant group into consecutive message runs so a tall
 * bubble can be virtualized row by row instead of mounting all parts at once.
 * Zero-height parts (hidden reasoning) stay attached to a neighbouring run so
 * no segment renders only empty chrome. Returns null when the group should
 * remain a single row.
 */
export function splitMergedGroupMessages<T extends { id: number }>(
    messages: readonly T[],
    heightById: ReadonlyMap<number, number>,
    groupEstimatedHeight: number,
): T[][] | null {
    if (groupEstimatedHeight < MERGED_GROUP_SPLIT_MIN_HEIGHT_PX || messages.length < 2) {
        return null;
    }

    const segments: T[][] = [];
    let current: T[] = [];
    let currentHeight = 0;
    for (const message of messages) {
        current.push(message);
        currentHeight += heightById.get(message.id) ?? 0;
        if (currentHeight >= MERGED_GROUP_SEGMENT_TARGET_PX) {
            segments.push(current);
            current = [];
            currentHeight = 0;
        }
    }
    if (current.length > 0) {
        if (currentHeight === 0 && segments.length > 0) {
            segments[segments.length - 1].push(...current);
        } else {
            segments.push(current);
        }
    }

    return segments.length >= 2 ? segments : null;
}
