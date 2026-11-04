import { act, fireEvent, render } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@/__tests__/mocks/tauri";
import ChatUIInfomation from "./ChatUIInfomation";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("../hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
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

    it("desktop narrow screen still opens a separate config window", async () => {
        const listener = vi.fn();
        window.addEventListener("aipp:mobile-navigate", listener);
        render(<ChatUIInfomation isMobile />);
        await act(async () => { fireEvent.click(slot("chat-info-open-config")!); });
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_config_window");
        expect(listener).not.toHaveBeenCalled();
        window.removeEventListener("aipp:mobile-navigate", listener);
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
