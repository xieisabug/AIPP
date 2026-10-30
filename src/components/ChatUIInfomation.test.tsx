import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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
        (window as any).__setAppWindow = vi.fn();
    });
    afterEach(() => {
        delete (window as any).__setAppWindow;
    });

    it("desktop: shows artifacts/schedule and opens a separate config window", async () => {
        render(<ChatUIInfomation />);
        expect(slot("chat-info-open-artifacts")).toBeTruthy();
        expect(slot("chat-info-open-schedule")).toBeTruthy();
        await act(async () => {
            fireEvent.click(slot("chat-info-open-config")!);
        });
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("open_config_window");
        expect((window as any).__setAppWindow).not.toHaveBeenCalled();
    });

    it("mobile: hides multi-window entries and switches views in the single WebView", async () => {
        platformState.mobile = true;
        render(<ChatUIInfomation showArtifacts showSchedule />);
        expect(slot("chat-info-open-artifacts")).toBeNull();
        expect(slot("chat-info-open-schedule")).toBeNull();
        await act(async () => {
            fireEvent.click(slot("chat-info-open-config")!);
        });
        expect((window as any).__setAppWindow).toHaveBeenCalledWith("config");
        expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("open_config_window");
    });
});
