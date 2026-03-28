export const VIRTUAL_ROW_GAP_PX = 16;
export const VIRTUAL_OVERSCAN_PX = 1200;

export interface VirtualizedLayoutItem {
    key: string;
    estimatedHeight: number;
}

export interface VirtualizedLayoutSnapshot {
    heights: number[];
    tops: number[];
    totalHeight: number;
}

export function buildVirtualizedLayout(
    items: VirtualizedLayoutItem[],
    measuredHeights: Record<string, number>,
    rowGapPx: number = VIRTUAL_ROW_GAP_PX,
): VirtualizedLayoutSnapshot {
    const heights: number[] = [];
    const tops: number[] = [];
    let cursor = 0;

    items.forEach((item, index) => {
        const height = measuredHeights[item.key] ?? item.estimatedHeight;
        heights.push(height);
        tops.push(cursor);
        cursor += height;
        if (index < items.length - 1) {
            cursor += rowGapPx;
        }
    });

    return {
        heights,
        tops,
        totalHeight: cursor,
    };
}

function findFirstIntersectingIndex(
    tops: number[],
    heights: number[],
    offset: number,
): number {
    let low = 0;
    let high = tops.length - 1;
    let answer = tops.length;

    while (low <= high) {
        const mid = Math.floor((low + high) / 2);
        const rowBottom = tops[mid] + heights[mid];
        if (rowBottom >= offset) {
            answer = mid;
            high = mid - 1;
        } else {
            low = mid + 1;
        }
    }

    return answer;
}

export function findVisibleRange(
    layout: VirtualizedLayoutSnapshot,
    scrollTop: number,
    viewportHeight: number,
    overscanPx: number = VIRTUAL_OVERSCAN_PX,
): { startIndex: number; endIndex: number } {
    if (layout.tops.length === 0) {
        return { startIndex: 0, endIndex: 0 };
    }

    const startOffset = Math.max(0, scrollTop - overscanPx);
    const endOffset = scrollTop + viewportHeight + overscanPx;
    const startIndex = Math.max(
        0,
        Math.min(
            layout.tops.length - 1,
            findFirstIntersectingIndex(layout.tops, layout.heights, startOffset),
        ),
    );

    let endIndex = startIndex;
    while (
        endIndex < layout.tops.length
        && layout.tops[endIndex] < endOffset
    ) {
        endIndex += 1;
    }

    return {
        startIndex,
        endIndex: Math.max(startIndex + 1, Math.min(layout.tops.length, endIndex)),
    };
}

export function getCenteredScrollTopForIndex(
    layout: VirtualizedLayoutSnapshot,
    viewportHeight: number,
    index: number,
): number {
    if (index < 0 || index >= layout.tops.length) {
        return 0;
    }

    const maxScrollTop = Math.max(0, layout.totalHeight - viewportHeight);
    const targetTop = layout.tops[index];
    const targetHeight = layout.heights[index];
    const centered =
        targetTop - Math.max(0, (viewportHeight - targetHeight) / 2);

    return Math.max(0, Math.min(centered, maxScrollTop));
}
