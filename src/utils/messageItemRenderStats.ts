const renderCounts = new Map<number, number>();

function countingEnabled(): boolean {
    return import.meta.env.DEV
        || import.meta.env.MODE === "test"
        || typeof window.__AIPP_CHAT_PERF_CAPTURE__?.recordMessageItemRender === "function";
}

export function recordMessageItemRender(messageId: number) {
    window.__AIPP_CHAT_PERF_CAPTURE__?.recordMessageItemRender?.(messageId);
    if (!countingEnabled()) {
        return;
    }
    renderCounts.set(messageId, (renderCounts.get(messageId) ?? 0) + 1);
}

export function getMessageItemRenderCount(messageId: number): number {
    return renderCounts.get(messageId) ?? 0;
}

export function resetMessageItemRenderCounts() {
    renderCounts.clear();
    window.__AIPP_CHAT_PERF_CAPTURE__?.resetMessageItemRenderCounts?.();
}

export interface VirtualRowEstimateSummary {
    measuredRowCount: number;
    totalEstimatedHeight: number;
    totalMeasuredHeight: number;
    rows: Array<{
        key: string;
        estimatedHeight: number;
        measuredHeight: number;
        parts?: VirtualRowMessagePart[];
    }>;
}

export interface VirtualRowMessagePart {
    id: string;
    type: string;
    height: number;
}

declare global {
    interface Window {
        __AIPP_CHAT_PERF_CAPTURE__?: {
            recordVirtualRowHeight?: (
                key: string,
                height: number,
                estimatedHeight?: number,
                parts?: VirtualRowMessagePart[],
            ) => void;
            resetVirtualRowHeightDrift?: () => void;
            getVirtualRowHeightDrift?: () => Array<{
                key: string;
                changeCount: number;
                minHeight: number;
                maxHeight: number;
                delta: number;
                lastHeight: number;
            }>;
            recordMessageItemRender?: (messageId: number) => void;
            resetMessageItemRenderCounts?: () => void;
            getMessageItemRenderCounts?: () => Record<string, number>;
            getVirtualRowEstimateSummary?: () => VirtualRowEstimateSummary;
        };
    }
}
