import { useCallback, useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";

interface DataRefreshOptions {
    events: string[];
    refresh: (isCurrent: () => boolean) => void | Promise<void>;
    identity?: string | number;
    paused?: boolean;
    initial?: boolean;
    onInvalidate?: () => void;
    onError?: (error: unknown) => void;
}

/** Coalesce invalidations, serialize reads and discard superseded responses. */
export function useDataRefresh({ events, refresh, identity, paused = false, initial = false, onError, onInvalidate }: DataRefreshOptions) {
    const latest = useRef({ refresh, paused, onError, onInvalidate });
    latest.current = { refresh, paused, onError, onInvalidate };
    const requestRef = useRef<() => void>(() => {});
    const eventsKey = JSON.stringify(events);
    const pauseChangedRef = useRef<() => void>(() => {});

    useEffect(() => {
        let disposed = false;
        let revision = 0;
        let dirty = false;
        let running = false;
        let timer: ReturnType<typeof setTimeout> | undefined;
        const report = (error: unknown) => {
            if (latest.current.onError) latest.current.onError(error);
            else toast.error(`刷新数据失败: ${String(error)}`);
        };
        const run = async () => {
            timer = undefined;
            if (disposed || running || !dirty || latest.current.paused) return;
            running = true;
            dirty = false;
            const currentRevision = revision;
            const isCurrent = () => !disposed && revision === currentRevision && !latest.current.paused;
            try {
                await latest.current.refresh(isCurrent);
            } catch (error) {
                if (isCurrent()) report(error);
            } finally {
                running = false;
                if (!disposed && dirty && !latest.current.paused) timer = setTimeout(run, 100);
            }
        };
        const request = () => {
            latest.current.onInvalidate?.();
            revision += 1;
            dirty = true;
            if (timer) clearTimeout(timer);
            timer = setTimeout(run, 100);
        };
        requestRef.current = request;
        pauseChangedRef.current = () => {
            if (latest.current.paused && running) { revision += 1; dirty = true; }
            if (!latest.current.paused && dirty) {
                if (timer) clearTimeout(timer);
                timer = setTimeout(run, 100);
            }
        };
        const subscriptions = (JSON.parse(eventsKey) as string[]).map((event) =>
            listen(event, request).then((unlisten) => {
                if (disposed) { unlisten(); return; }
                return unlisten;
            }).catch((error) => { if (!disposed) report(`订阅 ${event} 失败: ${String(error)}`); }),
        );
        if (initial) { revision += 1; dirty = true; latest.current.onInvalidate?.(); void run(); }
        return () => {
            disposed = true;
            revision += 1;
            if (timer) clearTimeout(timer);
            requestRef.current = () => {};
            pauseChangedRef.current = () => {};
            for (const subscription of subscriptions) void subscription.then((unlisten) => unlisten?.());
        };
    }, [eventsKey, identity, initial]);

    useEffect(() => { pauseChangedRef.current(); }, [paused]);

    return useCallback(() => requestRef.current(), []);
}
