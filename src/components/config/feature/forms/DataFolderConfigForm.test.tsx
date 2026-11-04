import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import { useForm } from "react-hook-form";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

import { DataFolderConfigForm } from "./DataFolderConfigForm";

const platformState = vi.hoisted(() => ({ value: "windows" }));

vi.mock("@/hooks/use-platform", () => ({
    usePlatform: () => platformState.value,
    useIsMobilePlatform: () => platformState.value === "android" || platformState.value === "ios",
}));

vi.mock("sonner", () => ({
    toast: { success: vi.fn(), error: vi.fn(), warning: vi.fn() },
}));

function status(overrides: Record<string, unknown> = {}) {
    return {
        mode: "self_hosted",
        server_url: "https://sync.example.com",
        token_configured: true,
        connected: true,
        running: true,
        syncing: false,
        paused: false,
        last_sync_at: null,
        last_error: null,
        pending_outbox_count: 0,
        pushing_outbox_count: 0,
        failed_outbox_count: 0,
        dead_letter_count: 0,
        needs_reset: false,
        conflict_resolved_count: 0,
        server_cursor: 12,
        dangling_fk_count: 0,
        secret_sync_state: "needs_passphrase",
        secret_sync_message: "需要输入同步口令才能同步 API Key",
        secret_pending_count: 1,
        duplicate_provider_conflicts: [],
        ...overrides,
    };
}

function Harness({ mode = "self_hosted" }: { mode?: string }) {
    const form = useForm({ defaultValues: { mode, server_url: "", token: "" } });
    return <DataFolderConfigForm form={form} />;
}

function setViewport(width: number, platform: string) {
    platformState.value = platform;
    window.innerWidth = width;
    window.dispatchEvent(new Event("resize"));
}

async function renderWith(initial: Record<string, unknown>) {
    mockInvokeHandler("get_sync_status", () => status(initial));
    render(<Harness />);
    await screen.findByTestId("secret-sync-status");
}

function setPassphraseCalls() {
    return vi.mocked(invoke).mock.calls.filter(([command]) => command === "set_sync_passphrase");
}

const viewports = [
    { name: "desktop", width: 1400, platform: "windows" },
    { name: "mobile", width: 390, platform: "android" },
];

describe.each(viewports)("DataFolderConfigForm encrypted API Key sync ($name)", ({ width, platform }) => {
    beforeEach(() => setViewport(width, platform));

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
        setViewport(1024, "windows");
    });

    it("shows the API Key sync row in self-hosted mode", async () => {
        await renderWith({});
        expect(screen.getByTestId("secret-sync-status").closest(".hidden")).toBeNull();
    });

    it("has a single save button that validates locally, then saves", async () => {
        await renderWith({});
        expect(screen.getByText("需要输入同步口令才能同步 API Key")).toBeTruthy();
        const save = screen.getByRole("button", { name: "保存口令" });
        expect((save as HTMLButtonElement).disabled).toBe(true);
        for (const gone of ["验证口令", "更换口令", "清除本机口令", "合并重复供应商"]) {
            expect(screen.queryByRole("button", { name: gone })).toBeNull();
        }

        const input = screen.getByLabelText("同步口令") as HTMLInputElement;
        expect(input.type).toBe("password");
        expect(input.className).toContain("w-full");
        fireEvent.change(input, { target: { value: "short" } });
        expect(screen.getByLabelText("同步口令")).toBe(input); // not remounted while typing
        fireEvent.click(screen.getByRole("button", { name: "保存口令" }));
        expect(await screen.findByText("同步口令至少需要 8 个字符")).toBeTruthy();
        expect(setPassphraseCalls()).toHaveLength(0);

        mockInvokeHandler("set_sync_passphrase", () =>
            status({ secret_sync_state: "ready", secret_sync_message: "API Key 已加密同步" }),
        );
        fireEvent.change(screen.getByLabelText("同步口令"), { target: { value: "shared passphrase" } });
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "保存口令" }));
        });
        await screen.findByText("API Key 已加密同步");
        expect(setPassphraseCalls()).toHaveLength(1);
        expect(setPassphraseCalls()[0][1]).toEqual({
            request: { passphrase: "shared passphrase", replace_existing: false },
        });
        expect((screen.getByLabelText("同步口令") as HTMLInputElement).value).toBe("");
        expect(screen.getByRole("button", { name: "保存口令" })).toBeTruthy();
    });

    it("asks before replacing when the passphrase does not match, and replaces only on confirm", async () => {
        await renderWith({ secret_sync_state: "mismatch", secret_sync_message: "同步口令已在其他设备更换，请输入新口令" });
        mockInvokeHandler("set_sync_passphrase", (args: unknown) => {
            const { request } = args as { request: { replace_existing: boolean } };
            if (!request.replace_existing) {
                throw "SYNC_PASSPHRASE_MISMATCH: 输入的口令与已有同步口令不一致";
            }
            return status({ secret_sync_state: "ready", secret_sync_message: "API Key 已加密同步" });
        });
        fireEvent.change(screen.getByLabelText("同步口令"), { target: { value: "brand new passphrase" } });
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "保存口令" }));
        });
        expect(await screen.findByText("输入的口令与已有同步口令不一致。继续将更换所有设备的同步口令，其他设备需要重新输入新口令。")).toBeTruthy();
        expect(screen.queryByText(/SYNC_PASSPHRASE_MISMATCH/)).toBeNull();
        expect(setPassphraseCalls()).toHaveLength(1);

        await act(async () => {
            fireEvent.click(await screen.findByRole("button", { name: "确认" }));
        });
        await waitFor(() => expect(setPassphraseCalls()).toHaveLength(2));
        expect(setPassphraseCalls()[1][1]).toEqual({
            request: { passphrase: "brand new passphrase", replace_existing: true },
        });
        await screen.findByText("API Key 已加密同步");
    });

    it("cancelling the mismatch dialog changes nothing", async () => {
        await renderWith({ secret_sync_state: "needs_passphrase" });
        mockInvokeHandler("set_sync_passphrase", () => {
            throw "SYNC_PASSPHRASE_MISMATCH: 输入的口令与已有同步口令不一致";
        });
        fireEvent.change(screen.getByLabelText("同步口令"), { target: { value: "typo passphrase" } });
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "保存口令" }));
        });
        await act(async () => {
            fireEvent.click(await screen.findByRole("button", { name: "取消" }));
        });
        expect(setPassphraseCalls()).toHaveLength(1);
        expect(screen.getByText("需要输入同步口令才能同步 API Key")).toBeTruthy();
    });

    it("names same-name providers whose keys conflict, without any merge button", async () => {
        await renderWith({ secret_sync_state: "ready", secret_sync_message: "API Key 已加密同步", duplicate_provider_conflicts: ["Relay"] });
        expect(screen.getByTestId("provider-conflict-status").textContent).toBe("同名供应商「Relay」的 API Key 不一致，未自动合并");
        expect(screen.queryByRole("button", { name: "合并重复供应商" })).toBeNull();
    });

    it("shows no conflict line when there is none", async () => {
        await renderWith({ secret_sync_state: "not_configured", secret_sync_message: "未设置口令，API Key 不同步" });
        expect(screen.queryByTestId("provider-conflict-status")).toBeNull();
    });

    it("hides the API Key sync controls in local mode", async () => {
        mockInvokeHandler("get_sync_status", () => status({ mode: "local" }));
        render(<Harness mode="local" />);
        await waitFor(() => expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "get_sync_status")).toBe(true));
        // ConfigForm keeps hidden rows mounted with the `hidden` class.
        const row = await screen.findByTestId("secret-sync-status");
        expect(row.closest(".hidden")).not.toBeNull();
    });
});
