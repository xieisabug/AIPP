import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import ButlerSortableList from "./ButlerSortableList";

it("should restore a dropped row's position when the list reopens with changed membership", () => {
    const key = "butler-sort-test";
    const storage = new Map<string, string>();
    vi.stubGlobal("localStorage", { getItem: (k: string) => storage.get(k) ?? null, setItem: (k: string, v: string) => storage.set(k, v) });
    const row = (id: string) => ({ id, label: id, content: <span>内容：{id}</span> });
    const list = (...ids: string[]) => <nav><ButlerSortableList rows={ids.map(row)} storageKey={key}/></nav>;
    const first = render(list("a", "b", "c"));
    const handle = screen.getByRole("button", { name: "调整顺序：c" });
    const hit = screen.getByText("内容：a");
    const hitTest = vi.spyOn(document, "elementFromPoint").mockReturnValue(hit);
    Object.assign(handle, { setPointerCapture: vi.fn(), hasPointerCapture: () => true });
    try {
        fireEvent.pointerDown(handle, { button: 0, isPrimary: true, pointerId: 1 });
        fireEvent.pointerUp(handle, { pointerId: 1, clientY: 0 });
        expect(first.container.querySelector("[data-butler-sort-id]")).toHaveAttribute("data-butler-sort-id", "c");
        first.unmount();
        const reopened = render(list("a", "new", "c"));
        expect(Array.from(reopened.container.querySelectorAll("[data-butler-sort-id]"), el => el.getAttribute("data-butler-sort-id")))
            .toEqual(["c", "a", "new"]);
    } finally {
        hitTest.mockRestore();
        vi.unstubAllGlobals();
    }
});
