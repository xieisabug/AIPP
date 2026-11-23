import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { FloatingWindowControls, WindowControls, WindowTitleBar } from "./WindowTitleBar";

const platformState = vi.hoisted(() => ({ platform: "windows" as string }));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => platformState.platform }));

const appWindow = vi.hoisted(() => ({
    maximized: false,
    resized: undefined as undefined | (() => void),
    minimize: vi.fn(async () => {}),
    toggleMaximize: vi.fn(async () => {}),
    close: vi.fn(async () => {}),
}));
vi.mock("@tauri-apps/api/window", () => ({
    getCurrentWindow: () => ({
        isMaximized: async () => appWindow.maximized,
        onResized: async (cb: () => void) => { appWindow.resized = cb; return () => {}; },
        minimize: appWindow.minimize,
        toggleMaximize: appWindow.toggleMaximize,
        close: appWindow.close,
    }),
}));

describe("WindowControls", () => {
    beforeEach(() => {
        platformState.platform = "windows";
        appWindow.maximized = false;
        vi.clearAllMocks();
    });

    it("should draw minimize / maximize / close on Windows and route them to the window", async () => {
        render(<WindowControls />);
        expect(document.querySelector('[data-aipp-slot="window-controls"]')).toHaveClass("aipp-window-controls-enter");

        fireEvent.click(screen.getByRole("button", { name: "最小化" }));
        fireEvent.click(screen.getByRole("button", { name: "最大化" }));
        fireEvent.click(screen.getByRole("button", { name: "关闭" }));
        expect(appWindow.minimize).toHaveBeenCalledOnce();
        expect(appWindow.toggleMaximize).toHaveBeenCalledOnce();
        expect(appWindow.close).toHaveBeenCalledOnce();
    });

    it("should switch to restore after the window is maximized", async () => {
        render(<WindowControls />);
        await screen.findByRole("button", { name: "最大化" });
        appWindow.maximized = true;
        await act(async () => { appWindow.resized?.(); });
        expect(await screen.findByRole("button", { name: "还原" })).toBeTruthy();
    });

    it("should pin the floating variant to the given center line and right inset", () => {
        render(<FloatingWindowControls centerY={42} right={32} />);
        const floating = document.querySelector('[data-aipp-slot="window-controls-floating"]') as HTMLElement;
        expect(floating.style.top).toBe("26px");
        expect(floating.style.right).toBe("32px");
        expect(floating).toContainElement(screen.getByRole("button", { name: "关闭" }));
    });

    it.each(["macos", "linux"])("should leave window buttons to the system on %s", (platform) => {
        platformState.platform = platform;
        render(<><WindowControls /><FloatingWindowControls centerY={26} right={12} /></>);
        expect(screen.queryByRole("button", { name: "关闭" })).toBeNull();
        expect(document.querySelector('[data-aipp-slot="window-controls-floating"]')).toBeNull();
    });
});

describe("WindowTitleBar", () => {
    it.each([
        ["macos", "92px"],
        ["windows", "12px"],
        ["linux", "12px"],
    ])("should reserve the left gap for %s", (platform, paddingLeft) => {
        platformState.platform = platform;
        render(<WindowTitleBar data-aipp-slot="bar" />);
        const bar = document.querySelector('[data-aipp-slot="bar"]') as HTMLElement;
        expect(bar).toHaveAttribute("data-tauri-drag-region");
        expect(bar.style.paddingLeft).toBe(paddingLeft);
    });

    it("should not render on mobile", () => {
        platformState.platform = "android";
        render(<WindowTitleBar data-aipp-slot="bar" />);
        expect(document.querySelector('[data-aipp-slot="bar"]')).toBeNull();
    });
});
