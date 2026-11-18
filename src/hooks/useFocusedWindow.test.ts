import { act, renderHook } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import { useFocusedWindow } from "./useFocusedWindow";

it("should allow marking replies read only while the window is focused and visible", () => {
    const focus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    const hidden = vi.spyOn(document, "hidden", "get").mockReturnValue(false);
    try {
        const { result } = renderHook(useFocusedWindow);
        expect(result.current).toBe(false);
        focus.mockReturnValue(true);
        act(() => { window.dispatchEvent(new Event("focus")); });
        expect(result.current).toBe(true);
        hidden.mockReturnValue(true);
        act(() => { document.dispatchEvent(new Event("visibilitychange")); });
        expect(result.current).toBe(false);
    } finally {
        focus.mockRestore();
        hidden.mockRestore();
    }
});
