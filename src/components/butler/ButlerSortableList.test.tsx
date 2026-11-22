import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { expect, it, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerSortableList from "./ButlerSortableList";

it("should import the old order, persist a drop and refresh an open list after sync", async () => {
    const key = "butler-contact-order";
    const storage = new Map([[key, JSON.stringify(["group:2", "assistant:1", "assistant:3"])]]);
    vi.stubGlobal("localStorage", { getItem: (k: string) => storage.get(k) ?? null, removeItem: (k: string) => storage.delete(k) });
    let saved: string[] | undefined;
    const save = vi.fn(args => { saved = args.order; });
    mockInvokeHandler("get_butler_contact_order", args => saved ??= args?.legacy as string[]);
    mockInvokeHandler("save_butler_contact_order", save);
    const row = (id: string) => ({ id, label: id, content: <span>{id}</span> });
    const list = (...ids: string[]) => <nav><ButlerSortableList rows={ids.map(row)} /></nav>;
    const display = (node: HTMLElement) => Array.from(node.querySelectorAll("[data-butler-sort-id]"), el => el.getAttribute("data-butler-sort-id"));
    const first = render(list("assistant:1", "assistant:3", "group:2"));
    const handle = screen.getByRole("button", { name: "调整顺序：assistant:3" });
    const hitTest = vi.spyOn(document, "elementFromPoint").mockReturnValue(screen.getByText("group:2"));
    try {
        await waitFor(() => expect(handle).toBeEnabled());
        expect(storage.has(key)).toBe(false);
        expect(display(first.container)).toEqual(["group:2", "assistant:1", "assistant:3"]);
        Object.assign(handle, { setPointerCapture: vi.fn(), hasPointerCapture: () => true });
        fireEvent.pointerDown(handle, { button: 0, isPrimary: true, pointerId: 1 });
        fireEvent.pointerUp(handle, { pointerId: 1, clientY: 0 });
        await waitFor(() => expect(save).toHaveBeenCalledWith({ order: ["assistant:3", "group:2", "assistant:1"] }));
        first.unmount();
        const reopened = render(list("assistant:1", "group:4", "assistant:3"));
        await waitFor(() => expect(display(reopened.container)).toEqual(["assistant:3", "assistant:1", "group:4"]));
        saved = ["group:4", "assistant:3", "assistant:1"];
        await act(() => emit("feature_config_changed", {}));
        await waitFor(() => expect(display(reopened.container)).toEqual(saved));
        fireEvent.keyDown(screen.getByRole("button", { name: "调整顺序：assistant:1" }), { key: "ArrowUp" });
        await waitFor(() => expect(saved).toEqual(["group:4", "assistant:1", "assistant:3"]));
        reopened.unmount();
    } finally {
        hitTest.mockRestore();
        vi.unstubAllGlobals();
    }
});
