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
        secret_sync_message: "需要输入同步口令才能同步 API Key（其他设备已启用加密同步），在此之前本机 API Key 保持不变",
        secret_pending_count: 1,
        duplicate_provider_count: 0,
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

    it("asks for the passphrase, validates it locally, then verifies it", async () => {
        await renderWith({});
        expect(screen.getByText(/需要输入同步口令才能同步 API Key/)).toBeTruthy();
        const verify = screen.getByRole("button", { name: "验证口令" });
        expect((verify as HTMLButtonElement).disabled).toBe(true);

        const input = screen.getByLabelText("同步口令") as HTMLInputElement;
        expect(input.type).toBe("password");
        expect(input.className).toContain("w-full");
        fireEvent.change(input, { target: { value: "short" } });
        expect(screen.getByLabelText("同步口令")).toBe(input); // not remounted while typing
        fireEvent.click(screen.getByRole("button", { name: "验证口令" }));
        expect(await screen.findByText("同步口令至少需要 8 个字符")).toBeTruthy();
        expect(setPassphraseCalls()).toHaveLength(0);

        mockInvokeHandler("set_sync_passphrase", () =>
            status({ secret_sync_state: "ready", secret_sync_message: "已启用 API Key 端到端加密同步（服务器只保存密文）" }),
        );
        fireEvent.change(screen.getByLabelText("同步口令"), { target: { value: "shared passphrase" } });
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "验证口令" }));
        });
        await screen.findByText("已启用 API Key 端到端加密同步（服务器只保存密文）");
        expect(setPassphraseCalls()[0][1]).toEqual({
            request: { passphrase: "shared passphrase", replace_existing: false },
        });
        expect((screen.getByLabelText("同步口令") as HTMLInputElement).value).toBe("");
        expect(screen.getByRole("button", { name: "清除本机口令" })).toBeTruthy();
    });

    it("shows a wrong passphrase error and keeps the paused status", async () => {
        await renderWith({});
        mockInvokeHandler("set_sync_passphrase", () => {
            throw "同步口令错误：与其他设备设置的同步口令不一致。";
        });
        fireEvent.change(screen.getByLabelText("同步口令"), { target: { value: "wrong passphrase" } });
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "验证口令" }));
        });
        expect(await screen.findByText(/同步口令错误/)).toBeTruthy();
        expect(screen.getByText(/需要输入同步口令才能同步 API Key/)).toBeTruthy();
    });

    it("replaces the passphrase only after confirmation", async () => {
        await renderWith({ secret_sync_state: "mismatch", secret_sync_message: "同步口令与其他设备不一致" });
        mockInvokeHandler("set_sync_passphrase", () => status({ secret_sync_state: "ready", secret_sync_message: "已启用" }));
        fireEvent.change(screen.getByLabelText("同步口令"), { target: { value: "brand new passphrase" } });
        fireEvent.click(screen.getByRole("button", { name: "更换口令" }));
        expect(setPassphraseCalls()).toHaveLength(0);
        await act(async () => {
            fireEvent.click(await screen.findByRole("button", { name: "确认" }));
        });
        await waitFor(() => expect(setPassphraseCalls()).toHaveLength(1));
        expect(setPassphraseCalls()[0][1]).toEqual({
            request: { passphrase: "brand new passphrase", replace_existing: true },
        });
    });

    it("offers to merge duplicate providers left by older syncs", async () => {
        await renderWith({ secret_sync_state: "not_configured", secret_sync_message: "未设置同步口令", duplicate_provider_count: 1 });
        expect(screen.getByRole("button", { name: "设置口令" })).toBeTruthy();
        expect(screen.queryByRole("button", { name: "更换口令" })).toBeNull();
        expect(screen.getByText(/发现 1 个同名重复的供应商/)).toBeTruthy();
        mockInvokeHandler("merge_duplicate_sync_providers", () => status({ duplicate_provider_count: 0 }));
        fireEvent.click(screen.getByRole("button", { name: "合并重复供应商" }));
        await act(async () => {
            fireEvent.click(await screen.findByRole("button", { name: "确认" }));
        });
        await waitFor(() => expect(screen.queryByText(/同名重复的供应商/)).toBeNull());
        expect(vi.mocked(invoke).mock.calls.some(([command]) => command === "merge_duplicate_sync_providers")).toBe(true);
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
