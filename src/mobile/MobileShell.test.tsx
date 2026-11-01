import { useEffect, useState } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import MobileShell from "./MobileShell";
import { navigateMobile, previewMobile } from "./navigation";

const mounts = vi.hoisted(() => ({ chat: 0 }));
vi.mock("../windows/ChatUIWindow", () => ({
    default: () => {
        const [draft, setDraft] = useState("");
        useEffect(() => { mounts.chat += 1; }, []);
        return <input aria-label="chat draft" value={draft} onChange={event => setDraft(event.target.value)} />;
    },
}));
vi.mock("../windows/ConfigWindow", () => ({ default: () => <div>config page</div> }));
vi.mock("./MobileArtifactPreview", () => ({
    default: ({ preview }: { preview: object }) => {
        const [count, setCount] = useState(0);
        useEffect(() => { setCount(0); }, [preview]);
        return <button onClick={() => setCount(value => value + 1)}>preview count {count}</button>;
    },
}));

describe("MobileShell navigation", () => {
    it("preserves the mounted chat and draft across navigation and back", async () => {
        mounts.chat = 0;
        render(<MobileShell />);
        const draft = await screen.findByLabelText("chat draft");
        fireEvent.change(draft, { target: { value: "unfinished" } });
        act(() => navigateMobile("config"));
        expect(await screen.findByText("config page")).toBeVisible();
        expect(draft).not.toBeVisible();
        act(() => { expect(window.__aippMobileBack?.()).toBe(true); });
        expect(draft).toBeVisible();
        expect(draft).toHaveValue("unfinished");
        act(() => navigateMobile("config"));
        act(() => navigateMobile("chat_ui"));
        expect(draft).toBeVisible();
        expect(mounts.chat).toBe(1);
        expect(window.__aippMobileBack?.()).toBe(false);
    });

    it("returns through the navigation stack and rejects invalid routes", async () => {
        render(<MobileShell />);
        const draft = await screen.findByLabelText("chat draft");
        expect(window.__aippMobileBack?.()).toBe(false);
        act(() => window.dispatchEvent(new CustomEvent("aipp:mobile-navigate", { detail: "invalid" })));
        expect(draft).toBeVisible();
        expect(screen.queryByText("config page")).toBeNull();
        act(() => navigateMobile("config"));
        expect(await screen.findByText("config page")).toBeVisible();
        act(() => navigateMobile("config"));
        act(() => { expect(window.__aippMobileBack?.()).toBe(true); });
        expect(draft).toBeVisible();
        expect(window.__aippMobileBack?.()).toBe(false);
    });

    it("sends Escape to the top visible panel before returning a page", async () => {
        render(<MobileShell />);
        await screen.findByLabelText("chat draft");
        act(() => navigateMobile("config"));
        const config = await screen.findByText("config page");
        const panel = document.createElement("div");
        panel.setAttribute("data-mobile-panel", "");
        panel.getClientRects = () => [new DOMRect(0, 0, 10, 10)] as unknown as DOMRectList;
        const handler = vi.fn();
        panel.addEventListener("keydown", handler);
        document.body.append(panel);
        act(() => { expect(window.__aippMobileBack?.()).toBe(true); });
        expect(handler).toHaveBeenCalledOnce();
        expect((handler.mock.calls[0][0] as KeyboardEvent).key).toBe("Escape");
        expect(config).toBeVisible();
        panel.remove();
        act(() => { expect(window.__aippMobileBack?.()).toBe(true); });
        expect(screen.getByLabelText("chat draft")).toBeVisible();
    });

    it("handles back immediately after navigation in the same batch", async () => {
        render(<MobileShell />);
        const draft = await screen.findByLabelText("chat draft");
        act(() => {
            navigateMobile("config");
            expect(window.__aippMobileBack?.()).toBe(true);
        });
        expect(draft).toBeVisible();
        expect(window.__aippMobileBack?.()).toBe(false);
    });

    it("preserves preview state when the same content is reopened", async () => {
        render(<MobileShell />);
        await screen.findByLabelText("chat draft");
        const preview = { language: "react", code: "example", title: "Example" };
        act(() => previewMobile(preview));
        fireEvent.click(await screen.findByRole("button", { name: "preview count 0" }));
        expect(screen.getByRole("button", { name: "preview count 1" })).toBeVisible();
        act(() => { expect(window.__aippMobileBack?.()).toBe(true); });
        act(() => previewMobile({ ...preview }));
        expect(screen.getByRole("button", { name: "preview count 1" })).toBeVisible();
    });
});
