import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AntigravityLogin from "./AntigravityLogin";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), openUrl: vi.fn(), onmessage: undefined as undefined | ((url: string) => void) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, Channel: class {
    set onmessage(handler: (url: string) => void) { mocks.onmessage = handler; }
} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: mocks.openUrl }));

describe("Antigravity sign-in", () => {
    beforeEach(() => { mocks.invoke.mockReset(); mocks.openUrl.mockReset().mockResolvedValue(undefined); mocks.onmessage = undefined; });
    it("should show the browser link without claiming success when authentication is pending", async () => {
        let complete!: () => void;
        mocks.invoke.mockImplementation(() => new Promise<void>(resolve => { complete = resolve; }));
        render(<AntigravityLogin providerId="7" settings={() => ({ agent_cli_command: "custom-agy" })} />);
        fireEvent.click(screen.getByRole("button", { name: "登录 Antigravity" }));
        expect(mocks.invoke).toHaveBeenCalledWith("login_antigravity", expect.objectContaining({ providerId: 7, settings: { agent_cli_command: "custom-agy" } }));
        act(() => mocks.onmessage?.("https://accounts.google.com/synthetic"));
        expect(screen.queryByText(/已确认认证成功/)).not.toBeInTheDocument();
        expect(mocks.openUrl).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "在浏览器中继续登录" }));
        expect(mocks.openUrl).toHaveBeenCalledWith("https://accounts.google.com/synthetic");
        await act(async () => complete());
        expect(screen.getByText(/已确认认证成功/)).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "在浏览器中继续登录" })).not.toBeInTheDocument();
    });
    it("should keep the native diagnostic when authentication fails", async () => {
        mocks.invoke.mockRejectedValue("authenticate: account disabled (code=-32000)");
        render(<AntigravityLogin providerId="7" settings={() => ({})} />);
        fireEvent.click(screen.getByRole("button", { name: "登录 Antigravity" }));
        expect(await screen.findByText(/account disabled/)).toBeInTheDocument();
        expect(screen.queryByText(/已确认认证成功/)).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "登录 Antigravity" })).toBeEnabled();
    });
    it("should cancel the owned login when the user cancels or leaves the provider", async () => {
        mocks.invoke.mockImplementation((name: string) => name === "login_antigravity" ? new Promise(() => {}) : Promise.resolve());
        const view = render(<AntigravityLogin providerId="7" settings={() => ({})} />);
        fireEvent.click(screen.getByRole("button", { name: "登录 Antigravity" }));
        fireEvent.click(screen.getByRole("button", { name: "取消登录" }));
        await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("cancel_antigravity_login", { providerId: 7 }));
        mocks.invoke.mockClear(); view.unmount();
        expect(mocks.invoke).toHaveBeenCalledWith("cancel_antigravity_login", { providerId: 7 });
    });
});
