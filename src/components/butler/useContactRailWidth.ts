import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";

export const BUTLER_CONTACT_RAIL_WIDTH_KEY = "butler-contact-rail-width";
export const MIN_CONTACT_RAIL_WIDTH = 180;
export const BASE_MAX_CONTACT_RAIL_WIDTH = 260;
/** 在原上限 260px 基础上增加 100% 的可拖拽扩大量（最高 520px）。 */
export const MAX_CONTACT_RAIL_WIDTH = BASE_MAX_CONTACT_RAIL_WIDTH * 2;
const DEFAULT_MIN_CONTACT_RAIL_WIDTH = 200;
const MIN_MAIN_PANE_WIDTH = 280;
const KEYBOARD_STEP = 16;
const KEYBOARD_FAST_STEP = 48;
const DRAG_THRESHOLD_PX = 2;

export function getEffectiveMaxContactRailWidth(viewportWidth = typeof window !== "undefined" ? window.innerWidth : 0): number {
    if (viewportWidth > 0) {
        return Math.max(MIN_CONTACT_RAIL_WIDTH, Math.min(MAX_CONTACT_RAIL_WIDTH, viewportWidth - MIN_MAIN_PANE_WIDTH));
    }
    return MAX_CONTACT_RAIL_WIDTH;
}

export function getDefaultContactRailWidth(viewportWidth = typeof window !== "undefined" ? window.innerWidth : 0): number {
    if (viewportWidth > 0) {
        return Math.min(BASE_MAX_CONTACT_RAIL_WIDTH, Math.max(DEFAULT_MIN_CONTACT_RAIL_WIDTH, Math.round(viewportWidth * 0.23)));
    }
    return 240;
}

export function clampContactRailWidth(width: number, maxWidth = getEffectiveMaxContactRailWidth()): number {
    return Math.min(maxWidth, Math.max(getDefaultContactRailWidth(), Math.round(width)));
}

function readStoredPreferred(): number | null {
    try {
        const raw = localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY);
        if (!raw) return null;
        const parsed = Number(raw);
        if (!Number.isFinite(parsed) || parsed <= 0) return null;
        return Math.min(MAX_CONTACT_RAIL_WIDTH, Math.round(parsed));
    } catch {
        return null;
    }
}

function loadInitialRailWidth() {
    const maxWidth = getEffectiveMaxContactRailWidth();
    const preferred = readStoredPreferred();
    const minWidth = Math.min(getDefaultContactRailWidth(), maxWidth);
    const width = clampContactRailWidth(preferred ?? minWidth, maxWidth);
    return { minWidth, maxWidth, preferred, width };
}

export function useContactRailWidth() {
    const [initial] = useState(loadInitialRailWidth);
    const preferredWidthRef = useRef<number | null>(initial.preferred);
    const [maxWidth, setMaxWidth] = useState(initial.maxWidth);
    const [minWidth, setMinWidth] = useState(initial.minWidth);
    const [width, setWidth] = useState(initial.width);
    const [isResizing, setIsResizing] = useState(false);
    const dragCleanupRef = useRef<(() => void) | null>(null);
    const widthRef = useRef(width);
    widthRef.current = width;

    useEffect(() => {
        const handleWindowResize = () => {
            const nextMax = getEffectiveMaxContactRailWidth();
            const nextMin = Math.min(getDefaultContactRailWidth(), nextMax);
            setMinWidth(nextMin);
            setMaxWidth(nextMax);
            setWidth(clampContactRailWidth(preferredWidthRef.current ?? nextMin, nextMax));
        };
        window.addEventListener("resize", handleWindowResize);
        return () => window.removeEventListener("resize", handleWindowResize);
    }, []);

    useEffect(() => () => dragCleanupRef.current?.(), []);

    const persistWidth = useCallback((value: number) => {
        try {
            localStorage.setItem(BUTLER_CONTACT_RAIL_WIDTH_KEY, String(value));
        } catch {
            // ignore storage errors
        }
    }, []);

    const clearStoredWidth = useCallback(() => {
        try {
            localStorage.removeItem(BUTLER_CONTACT_RAIL_WIDTH_KEY);
        } catch {
            // ignore storage errors
        }
    }, []);

    const handleResizeStart = useCallback((event: ReactPointerEvent<HTMLDivElement>) => {
        if (event.pointerType === "mouse" && event.button !== 0) return;
        event.preventDefault();
        dragCleanupRef.current?.();

        const startX = event.clientX;
        const startWidth = widthRef.current;
        const handle = event.currentTarget;
        const pointerId = event.pointerId;
        let previewWidth = startWidth;
        let frame: number | null = null;
        let moved = false;
        try {
            event.currentTarget.setPointerCapture(event.pointerId);
        } catch {
            // 不支持指针捕获时，仍靠 document 监听完成拖拽。
        }

        const previousCursor = document.body.style.cursor;
        const previousUserSelect = document.body.style.userSelect;
        document.body.style.cursor = "ew-resize";
        document.body.style.userSelect = "none";
        setIsResizing(true);

        const updatePreview = (clientX: number) => {
            const delta = clientX - startX;
            if (!moved && Math.abs(delta) <= DRAG_THRESHOLD_PX) return;
            moved = true;
            previewWidth = clampContactRailWidth(startWidth + delta, getEffectiveMaxContactRailWidth());
        };
        const handleMove = (moveEvent: PointerEvent) => {
            if (moveEvent.pointerId !== pointerId) return;
            updatePreview(moveEvent.clientX);
            if (!moved || frame !== null) return;
            frame = requestAnimationFrame(() => {
                frame = null;
                handle.style.transform = `translateX(${previewWidth - widthRef.current}px)`;
            });
        };

        let finished = false;
        const finish = () => {
            if (finished) return;
            finished = true;
            document.removeEventListener("pointermove", handleMove);
            document.removeEventListener("pointerup", handleEnd);
            document.removeEventListener("pointercancel", handleEnd);
            if (frame !== null) cancelAnimationFrame(frame);
            handle.style.transform = "";
            window.removeEventListener("blur", finish);
            document.body.style.cursor = previousCursor;
            document.body.style.userSelect = previousUserSelect;
            dragCleanupRef.current = null;
            setIsResizing(false);
            if (moved) {
                const finalWidth = clampContactRailWidth(previewWidth);
                preferredWidthRef.current = finalWidth;
                widthRef.current = finalWidth;
                setWidth(finalWidth);
                persistWidth(finalWidth);
            }
        };
        const handleEnd = (endEvent: PointerEvent) => {
            if (endEvent.pointerId !== pointerId) return;
            if (endEvent.type === "pointerup") updatePreview(endEvent.clientX);
            finish();
        };

        dragCleanupRef.current = finish;
        document.addEventListener("pointermove", handleMove);
        document.addEventListener("pointerup", handleEnd);
        document.addEventListener("pointercancel", handleEnd);
        window.addEventListener("blur", finish);
    }, [persistWidth]);

    const resetWidth = useCallback(() => {
        dragCleanupRef.current?.();
        preferredWidthRef.current = null;
        const nextMax = getEffectiveMaxContactRailWidth();
        const nextWidth = clampContactRailWidth(getDefaultContactRailWidth(), nextMax);
        widthRef.current = nextWidth;
        setMaxWidth(nextMax);
        setWidth(nextWidth);
        clearStoredWidth();
    }, [clearStoredWidth]);

    const handleResizeKeyDown = useCallback((event: ReactKeyboardEvent) => {
        const step = event.shiftKey ? KEYBOARD_FAST_STEP : KEYBOARD_STEP;
        const limit = getEffectiveMaxContactRailWidth();
        let nextWidth: number | null = null;
        if (event.key === "ArrowRight") {
            nextWidth = clampContactRailWidth(widthRef.current + step, limit);
        } else if (event.key === "ArrowLeft") {
            nextWidth = clampContactRailWidth(widthRef.current - step, limit);
        } else if (event.key === "Home") {
            nextWidth = minWidth;
        } else if (event.key === "End") {
            nextWidth = limit;
        }
        if (nextWidth === null) return;
        event.preventDefault();
        preferredWidthRef.current = nextWidth;
        widthRef.current = nextWidth;
        setWidth(nextWidth);
        setMaxWidth(limit);
        persistWidth(nextWidth);
    }, [minWidth, persistWidth]);

    return {
        width,
        minWidth,
        maxWidth,
        isResizing,
        handleResizeStart,
        handleResizeKeyDown,
        resetWidth,
    };
}
