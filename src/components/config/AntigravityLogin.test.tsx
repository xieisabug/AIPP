import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import AntigravityLogin from "./AntigravityLogin";

const loginSuccess = "Google 账号已登录";
const mocks = vi.hoisted(() => ({ invoke: vi.fn(), openUrl: vi.fn(), onmessage: undefined as undefined | ((url: string) => void) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, Channel: class {
    set onmessage(handler: (url: string) => void) { mocks.onmessage = handler; }
} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: mocks.openUrl }));

describe("Antigravity sign-in", () => {
    beforeEach(() => { mocks.invoke.mockReset(); mocks.openUrl.mockReset().mockResolvedValue(undefined); mocks.onmessage = undefined; });
    it("should show the browser link without claiming success when authentication is pending", async () => {
        let complete!: () => void;
        mocks.invoke.mockImplementation(name => name === "get_antigravity_login_status" ? Promise.resolve(false) : new Promise<void>(resolve => { complete = resolve; }));
        render(<AntigravityLogin providerId="7" settings={() => ({ agent_cli_command: "custom-agy" })} />);
        fireEvent.click(await screen.findByRole("button", { name: "登录 Antigravity" }));
        expect(mocks.invoke).toHaveBeenCalledWith("login_antigravity", expect.objectContaining({ providerId: 7, action: "login", settings: { agent_cli_command: "custom-agy" } }));
        act(() => mocks.onmessage?.("https://accounts.google.com/synthetic"));
        expect(screen.queryByText(loginSuccess)).not.toBeInTheDocument();
        expect(mocks.openUrl).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "在浏览器中继续登录" }));
        expect(mocks.openUrl).toHaveBeenCalledWith("https://accounts.google.com/synthetic");
        await act(async () => complete());
        expect(screen.getByText(loginSuccess)).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "登录 Antigravity" })).toBeNull();
        expect(screen.queryByRole("button", { name: "在浏览器中继续登录" })).not.toBeInTheDocument();
    });
    it("should keep the native diagnostic when authentication fails", async () => {
        mocks.invoke.mockImplementation(name => name === "get_antigravity_login_status" ? Promise.resolve(false) : Promise.reject("authenticate: account disabled (code=-32000)"));
        render(<AntigravityLogin providerId="7" settings={() => ({})} />);
        fireEvent.click(await screen.findByRole("button", { name: "登录 Antigravity" }));
        expect(await screen.findByText(/account disabled/)).toBeInTheDocument();
        expect(screen.queryByText(loginSuccess)).not.toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "登录 Antigravity" })).toBeEnabled();
    });
    it("should cancel the owned login when the user cancels or leaves the provider", async () => {
        mocks.invoke.mockImplementation((name: string) => name === "login_antigravity" ? new Promise(() => {}) : Promise.resolve());
        const view = render(<AntigravityLogin providerId="7" settings={() => ({})} />);
        fireEvent.click(await screen.findByRole("button", { name: "登录 Antigravity" }));
        fireEvent.click(screen.getByRole("button", { name: "取消登录" }));
        await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("cancel_antigravity_login", { providerId: 7 }));
        mocks.invoke.mockClear(); view.unmount();
        expect(mocks.invoke).toHaveBeenCalledWith("cancel_antigravity_login", { providerId: 7 });
    });
    it("should detect an existing sign-in on open and show login after logout", async () => {
        mocks.invoke.mockImplementation(name => Promise.resolve(name === "get_antigravity_login_status"));
        render(<AntigravityLogin providerId="7" settings={() => ({})} />);
        expect(screen.getByText("正在检测 Google 账号登录状态…")).toBeVisible();
        expect(await screen.findByText(loginSuccess)).toBeVisible();
        expect(screen.queryByRole("button", { name: "登录 Antigravity" })).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "退出登录" }));
        expect(await screen.findByRole("button", { name: "登录 Antigravity" })).toBeEnabled();
        expect(screen.queryByRole("button", { name: "退出登录" })).toBeNull();
    });
    it("should show a detection error separately from signed out and allow rechecking", async () => {
        mocks.invoke.mockRejectedValueOnce("spawn: agy missing").mockResolvedValue(true);
        render(<AntigravityLogin providerId="7" settings={() => ({})} />);
        expect(await screen.findByText("无法确认 Google 账号登录状态")).toBeVisible();
        expect(screen.getByRole("alert")).toHaveTextContent("agy missing");
        fireEvent.click(screen.getByRole("button", { name: "重新检测" }));
        expect(await screen.findByText(loginSuccess)).toBeVisible();
    });
    it("should show key configuration without checking a Google login for API-key authentication", () => {
        render(<AntigravityLogin providerId="7" settings={() => ({ antigravity_auth_method: "gemini-api-key", antigravity_api_key: "example" })} />);
        expect(screen.getByText("已填写 Gemini API Key")).toBeVisible();
        expect(screen.queryByRole("button", { name: "登录 Antigravity" })).toBeNull();
        expect(mocks.invoke).not.toHaveBeenCalled();
    });
});
