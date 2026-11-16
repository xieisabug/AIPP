import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
    BASE_MAX_CONTACT_RAIL_WIDTH,
    BUTLER_CONTACT_RAIL_WIDTH_KEY,
    MAX_CONTACT_RAIL_WIDTH,
    MIN_CONTACT_RAIL_WIDTH,
    clampContactRailWidth,
    getDefaultContactRailWidth,
    getEffectiveMaxContactRailWidth,
    useContactRailWidth,
} from "./useContactRailWidth";

function setViewport(width: number) {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: width, writable: true });
}

function Rail() {
    const rail = useContactRailWidth();
    return (
        <div
            role="separator"
            aria-label="调整联系人列表宽度"
            aria-valuemin={rail.minWidth}
            aria-valuemax={rail.maxWidth}
            aria-valuenow={rail.width}
            onPointerDown={rail.handleResizeStart}
            onDoubleClick={rail.resetWidth}
            onKeyDown={rail.handleResizeKeyDown}
        />
    );
}

function widthNow() {
    return Number(screen.getByRole("separator").getAttribute("aria-valuenow"));
}

function drag(from: number, to: number) {
    const handle = screen.getByRole("separator");
    fireEvent.pointerDown(handle, { button: 0, pointerType: "mouse", clientX: from, pointerId: 1 });
    fireEvent.pointerMove(document, { clientX: to, pointerId: 1 });
    fireEvent.pointerUp(document, { clientX: to, pointerId: 1 });
}

function memoryStorage() {
    const items = new Map<string, string>();
    return {
        getItem: (key: string) => items.get(key) ?? null,
        setItem: (key: string, value: string) => void items.set(key, value),
        removeItem: (key: string) => void items.delete(key),
        clear: () => items.clear(),
    };
}

describe("useContactRailWidth", () => {
    beforeEach(() => {
        vi.stubGlobal("localStorage", memoryStorage());
        setViewport(1280);
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    it("should allow one extra full expansion beyond the original 260px cap", () => {
        expect(MAX_CONTACT_RAIL_WIDTH).toBe(BASE_MAX_CONTACT_RAIL_WIDTH * 2);
        expect(clampContactRailWidth(900, MAX_CONTACT_RAIL_WIDTH)).toBe(520);
        expect(clampContactRailWidth(100, MAX_CONTACT_RAIL_WIDTH)).toBe(260);
        expect(getEffectiveMaxContactRailWidth(1600)).toBe(520);
        expect(getEffectiveMaxContactRailWidth(600)).toBe(320);
        expect(getEffectiveMaxContactRailWidth(400)).toBe(MIN_CONTACT_RAIL_WIDTH);
        expect(getDefaultContactRailWidth(1280)).toBe(260);
        expect(getDefaultContactRailWidth(1000)).toBe(230);
        expect(getDefaultContactRailWidth(800)).toBe(200);
    });

    it("should drag wider up to twice the original cap and remember it", () => {
        setViewport(1600);
        render(<Rail />);
        expect(widthNow()).toBe(260);

        drag(300, 700);

        expect(widthNow()).toBe(520);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBe("520");
    });

    it("should move only the divider until the pointer is released", () => {
        let paint: FrameRequestCallback = () => {};
        const requestFrame = vi.fn((callback: FrameRequestCallback) => {
            paint = callback;
            return 1;
        });
        vi.stubGlobal("requestAnimationFrame", requestFrame);
        render(<Rail />);
        const handle = screen.getByRole("separator");
        fireEvent.pointerDown(handle, { clientX: 260, pointerId: 1, button: 0 });
        fireEvent.pointerMove(document, { clientX: 300, pointerId: 1 });
        fireEvent.pointerMove(document, { clientX: 330, pointerId: 1 });
        expect(requestFrame).toHaveBeenCalledTimes(1);
        paint(0);
        expect(handle.style.transform).toBe("translateX(70px)");
        expect(widthNow()).toBe(260);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBeNull();
        fireEvent.pointerUp(document, { clientX: 340, pointerId: 1 });
        expect(handle.style.transform).toBe("");
        expect(widthNow()).toBe(340);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBe("340");
    });

    it("should drag narrower down to the minimum and remember it", () => {
        setViewport(1600);
        render(<Rail />);

        drag(400, 100);

        expect(widthNow()).toBe(260);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBe("260");
    });

    it("should keep a saved wider width when the window is temporarily narrow", () => {
        localStorage.setItem(BUTLER_CONTACT_RAIL_WIDTH_KEY, "520");
        setViewport(600);
        render(<Rail />);
        expect(widthNow()).toBe(320);

        setViewport(1600);
        fireEvent(window, new Event("resize"));

        expect(widthNow()).toBe(520);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBe("520");
    });

    it("should apply the current default as the minimum for an old narrow preference", () => {
        localStorage.setItem(BUTLER_CONTACT_RAIL_WIDTH_KEY, "180");
        render(<Rail />);
        for (const [viewport, expected] of [[1280, 260], [1000, 230], [800, 200], [400, 180]]) {
            setViewport(viewport);
            fireEvent(window, new Event("resize"));
            expect(widthNow()).toBe(expected);
            expect(screen.getByRole("separator").getAttribute("aria-valuemin")).toBe(String(expected));
        }
    });
    it("should follow the adaptive default until the user drags", () => {
        setViewport(1000);
        render(<Rail />);
        expect(widthNow()).toBe(230);

        setViewport(800);
        fireEvent(window, new Event("resize"));
        expect(widthNow()).toBe(200);

        setViewport(1600);
        fireEvent(window, new Event("resize"));
        expect(widthNow()).toBe(260);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBeNull();
    });

    it("should restore the adaptive width on double click", () => {
        localStorage.setItem(BUTLER_CONTACT_RAIL_WIDTH_KEY, "400");
        setViewport(1600);
        render(<Rail />);
        expect(widthNow()).toBe(400);

        fireEvent.doubleClick(screen.getByRole("separator"));

        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBeNull();
        expect(widthNow()).toBe(260);
    });

    it("should ignore a right-click and a pointer that does not move", () => {
        setViewport(1600);
        render(<Rail />);
        const handle = screen.getByRole("separator");

        fireEvent.pointerDown(handle, { button: 2, pointerType: "mouse", clientX: 100, pointerId: 1 });
        fireEvent.pointerMove(document, { clientX: 400, pointerId: 1 });
        fireEvent.pointerUp(document, { clientX: 400, pointerId: 1 });
        expect(widthNow()).toBe(260);

        fireEvent.pointerDown(handle, { button: 0, pointerType: "mouse", clientX: 100, pointerId: 1 });
        fireEvent.pointerUp(document, { clientX: 101, pointerId: 1 });
        expect(widthNow()).toBe(260);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBeNull();
    });

    it("should resize from the keyboard and remember the width", () => {
        setViewport(1600);
        render(<Rail />);
        const handle = screen.getByRole("separator");

        fireEvent.keyDown(handle, { key: "ArrowRight" });
        expect(widthNow()).toBe(276);
        fireEvent.keyDown(handle, { key: "ArrowRight", shiftKey: true });
        expect(widthNow()).toBe(324);
        fireEvent.keyDown(handle, { key: "ArrowLeft" });
        expect(widthNow()).toBe(308);
        fireEvent.keyDown(handle, { key: "Home" });
        expect(widthNow()).toBe(260);
        fireEvent.keyDown(handle, { key: "End" });
        expect(widthNow()).toBe(520);
        expect(localStorage.getItem(BUTLER_CONTACT_RAIL_WIDTH_KEY)).toBe("520");
    });

    it("should ignore a stored width that is not a number", () => {
        localStorage.setItem(BUTLER_CONTACT_RAIL_WIDTH_KEY, "wide");
        setViewport(1000);
        render(<Rail />);
        expect(widthNow()).toBe(230);
    });
});
