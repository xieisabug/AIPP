import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NativeAgentEnvironment, NativeAgentEnvironmentState } from "@/hooks/feature/useNativeAgentEnvironment";
import { CursorLogin } from "./CursorLogin";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), openUrl: vi.fn(), onmessage: undefined as undefined | ((url: string) => void) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke, Channel: class {
    set onmessage(handler: (url: string) => void) { mocks.onmessage = handler; }
} }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: mocks.openUrl }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));

function state(tool: Partial<NativeAgentEnvironment["tool"]> = {}): NativeAgentEnvironmentState {
    const environment = {
        driver: "cursor",
        node: { configured: "", required: "22.19.0", selected: null, candidates: [] },
        tool: { kind: "sdk", display_name: "Cursor SDK", default_command: "", configured: "", candidates: [], install_commands: [], docs_url: "",
            can_update: true, managed_directory: null, warnings: [], uses_api_key: false, account: { signed_in: false, email: null, expires_at_ms: null, expired: false },
            selected: { path: "C:/sdk", source: "AIPP 管理", version: "1.3.1", ok: true, warning: null, error: null }, ...tool },
        working_directory: { configured: "", resolved: "", exists: true },
    } as NativeAgentEnvironment;
    return { environment, detecting: false, detectError: null, detect: vi.fn().mockResolvedValue(null), currentVersion: "1.3.1", updateInfo: null,
        checkingUpdate: false, checkError: null, checkUpdate: vi.fn(), updating: false, updateLog: "", updateError: null, runUpdate: vi.fn() };
}

describe("CursorLogin", () => {
    beforeEach(() => { mocks.invoke.mockReset(); mocks.openUrl.mockReset().mockResolvedValue(undefined); mocks.onmessage = undefined; });

    it("should sign in through the browser link and refresh detection when Cursor confirms", async () => {
        let complete!: () => void;
        mocks.invoke.mockImplementation((command: string) => command === "login_cursor" ? new Promise<void>((resolve) => { complete = resolve; }) : Promise.resolve());
        const current = state();
        render(<CursorLogin providerId="9" state={current} settings={() => ({ agent_sdk_directory: "C:/sdk" })} />);
        fireEvent.click(screen.getByRole("button", { name: /登录 Cursor/ }));
        expect(mocks.invoke).toHaveBeenCalledWith("login_cursor", expect.objectContaining({ providerId: 9, settings: { agent_sdk_directory: "C:/sdk" } }));
        act(() => mocks.onmessage?.("https://cursor.com/loginDeepControl?challenge=1"));
        fireEvent.click(screen.getByRole("button", { name: /在浏览器中继续登录/ }));
        expect(mocks.openUrl).toHaveBeenCalledWith("https://cursor.com/loginDeepControl?challenge=1");
        await act(async () => complete());
        await waitFor(() => expect(current.detect).toHaveBeenCalled());
    });

    it("should show the signed-in account and sign out when asked", async () => {
        mocks.invoke.mockResolvedValue(undefined);
        const current = state({ account: { signed_in: true, email: "u@example.com", expires_at_ms: Date.UTC(2027, 1, 1), expired: false } });
        render(<CursorLogin providerId="9" state={current} settings={() => ({})} />);
        expect(screen.getByText(/已登录 u@example.com/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "退出登录" }));
        await waitFor(() => expect(mocks.invoke).toHaveBeenCalledWith("logout_cursor", { providerId: 9 }));
        expect(current.detect).toHaveBeenCalled();
    });

    it("should keep the native error visible when sign-in fails", async () => {
        mocks.invoke.mockRejectedValue("Cursor sign-in timed out after 5 minutes [provider_id=9]");
        render(<CursorLogin providerId="9" state={state()} settings={() => ({})} />);
        fireEvent.click(screen.getByRole("button", { name: /登录 Cursor/ }));
        expect(await screen.findByRole("alert")).toHaveTextContent("timed out after 5 minutes");
    });

    it("should require the SDK before signing in and defer to a configured API key", () => {
        const view = render(<CursorLogin providerId="9" state={state({ selected: { path: "C:/u", source: "工作目录", version: null, ok: false, warning: null, error: "没有安装" } })} settings={() => ({})} />);
        expect(screen.getByRole("button", { name: /登录 Cursor/ })).toBeDisabled();
        expect(screen.getByText(/请先在上方安装/)).toBeInTheDocument();
        view.unmount();
        render(<CursorLogin providerId="9" state={state({ uses_api_key: true })} settings={() => ({})} />);
        expect(screen.queryByRole("button", { name: /登录 Cursor/ })).not.toBeInTheDocument();
        expect(screen.getByText(/将优先使用它/)).toBeInTheDocument();
    });

    it("should cancel the pending sign-in when the provider form closes", () => {
        mocks.invoke.mockImplementation((command: string) => command === "login_cursor" ? new Promise(() => {}) : Promise.resolve());
        const view = render(<CursorLogin providerId="9" state={state()} settings={() => ({})} />);
        fireEvent.click(screen.getByRole("button", { name: /登录 Cursor/ }));
        mocks.invoke.mockClear();
        view.unmount();
        expect(mocks.invoke).toHaveBeenCalledWith("cancel_native_agent_login", { providerId: 9 });
    });
});
