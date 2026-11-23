import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ChatUIInfomation from "./ChatUIInfomation";

const platformState = vi.hoisted(() => ({ mobile: false, platform: "windows" as string }));
vi.mock("../hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
    usePlatform: () => platformState.platform,
}));
vi.mock("../lib/mobileUnsupported", () => ({
    useFeatureAvailableOnPlatform: () => !platformState.mobile,
}));
vi.mock("../contexts/AntiLeakageContext", () => ({
    useAntiLeakage: () => ({ enabled: false, isRevealed: false, toggleReveal: () => undefined }),
}));
vi.mock("./AnimatedLogo", () => ({ default: () => <div data-testid="logo" /> }));

const slot = (name: string) => document.querySelector(`[data-aipp-slot="${name}"]`) as HTMLElement | null;

describe("ChatUIInfomation platform gating", () => {
    beforeEach(() => {
        platformState.mobile = false;
        platformState.platform = "windows";
        vi.mocked(invoke).mockClear();
    });

    it("desktop: shows artifacts/schedule and opens a separate config window", async () => {
        render(<ChatUIInfomation />);
        expect(slot("chat-info-open-artifacts")).toBeTruthy();
        expect(slot("chat-info-open-schedule")).toBeTruthy();
        await act(async () => {
            fireEvent.click(slot("chat-info-open-config")!);
        });
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_config_window");
    });

    it("should put tools beside the logo below the macOS traffic-light row", () => {
        platformState.platform = "macos";
        render(<ChatUIInfomation />);
        const titleBar = slot("chat-info-titlebar")!;
        expect(titleBar).toHaveAttribute("data-tauri-drag-region");
        expect(titleBar.style.paddingLeft).toBe("92px");
        const brand = slot("chat-info-brand")!;
        expect(brand).toHaveAttribute("data-tauri-drag-region");
        expect(brand).toContainElement(screen.getByTestId("logo"));
        expect(brand).toContainElement(slot("chat-info-open-config"));
        expect(brand).toContainElement(slot("chat-info-open-artifacts"));
        expect(brand).toContainElement(slot("chat-info-open-schedule"));
        expect(slot("chat-info-open-config")).not.toHaveAttribute("data-tauri-drag-region");
    });

    it("should place search and new conversation beside the traffic lights on macOS only", () => {
        platformState.platform = "macos";
        const { rerender } = render(<ChatUIInfomation titleBarActions={<button>新对话</button>} />);
        expect(slot("chat-info-titlebar")).toContainElement(screen.getByRole("button", { name: "新对话" }));

        platformState.platform = "windows";
        rerender(<ChatUIInfomation titleBarActions={<button>新对话</button>} />);
        expect(screen.queryByRole("button", { name: "新对话" })).toBeNull();
    });

    it("should keep logo and tools in one row off macOS", () => {
        render(<ChatUIInfomation />);
        const bar = slot("chat-info-bar")!;
        expect(slot("chat-info-titlebar")).toBeNull();
        expect(bar).not.toHaveAttribute("data-tauri-drag-region");
        expect(bar).toContainElement(screen.getByTestId("logo"));
        expect(bar).toContainElement(slot("chat-info-open-config"));
    });

    it("desktop narrow screen still opens a separate config window", async () => {
        const listener = vi.fn();
        window.addEventListener("aipp:mobile-navigate", listener);
        render(<ChatUIInfomation isMobile />);
        await act(async () => { fireEvent.click(slot("chat-info-open-config")!); });
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_config_window");
        expect(listener).not.toHaveBeenCalled();
        window.removeEventListener("aipp:mobile-navigate", listener);
    });

    it("should show a ready update beside the desktop settings button", async () => {
        mockInvokeHandler("get_app_update_status", () => ({ revision: 1, version: "2.0", phase: "ready" }));
        render(<ChatUIInfomation />);
        const update = await screen.findByRole("button", { name: /点击安装更新/ });
        expect(slot("chat-info-open-config")?.nextElementSibling).toBe(update);
    });

    it("mobile: opens collections and config in the single WebView", async () => {
        platformState.mobile = true;
        const listener = vi.fn();
        window.addEventListener("aipp:mobile-navigate", listener);
        render(<ChatUIInfomation showArtifacts showSchedule />);
        expect(slot("chat-info-open-artifacts")).toBeTruthy();
        expect(slot("chat-info-open-schedule")).toBeNull();
        fireEvent.click(slot("chat-info-open-artifacts")!);
        expect((listener.mock.calls[0][0] as CustomEvent).detail).toBe("collections");
        await act(async () => {
            fireEvent.click(slot("chat-info-open-config")!);
        });
        expect(listener).toHaveBeenCalledTimes(2);
        expect((listener.mock.calls[1][0] as CustomEvent).detail).toBe("config");
        expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("open_config_window");
        window.removeEventListener("aipp:mobile-navigate", listener);
    });

    it.each(["chat-info-open-config", "chat-info-open-artifacts"])("mobile calls onNavigate when opening %s", (name) => {
        platformState.mobile = true;
        const onNavigate = vi.fn();
        render(<ChatUIInfomation onNavigate={onNavigate} />);
        fireEvent.click(slot(name)!);
        expect(onNavigate).toHaveBeenCalledOnce();
    });
});
