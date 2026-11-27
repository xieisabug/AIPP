import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler } from "@/__tests__/mocks/tauri";
import type { AppUpdateStatus } from "@/data/Update";
import { AppUpdateNotice } from "./AppUpdateNotice";

const downloading: AppUpdateStatus = {
    revision: 1, phase: "downloading", version: "2.0.0", body: null,
    downloaded: 512, total: 1024, error: null,
};
const ready: AppUpdateStatus = { ...downloading, revision: 2, phase: "ready", downloaded: 1024 };

describe("AppUpdateNotice", () => {
    const install = vi.fn();
    beforeEach(() => {
        install.mockClear();
        mockInvokeHandler("get_platform", () => "windows");
        mockInvokeHandler("get_app_update_status", () => downloading);
        mockInvokeHandler("install_app_update", install);
    });

    it("should show the current download after leaving and reopening the window", async () => {
        const first = render(<AppUpdateNotice rounded />);
        fireEvent.click(await screen.findByRole("button", { name: /正在下载/ }));
        expect(screen.getByText(/50%/)).toBeVisible();
        expect(screen.getByRole("progressbar", { name: "下载进度" })).toHaveAttribute("aria-valuenow", "50");
        first.unmount();

        mockInvokeHandler("get_app_update_status", () => ({ ...downloading, downloaded: 768 }));
        render(<AppUpdateNotice />);
        fireEvent.click(await screen.findByRole("button", { name: /正在下载/ }));
        expect(screen.getByText(/75%/)).toBeVisible();
        await act(() => emit("app-update-status", ready));
        const installButton = await screen.findByRole("button", { name: "立即安装" });
        expect(install).not.toHaveBeenCalled();
        fireEvent.click(installButton);
        expect(install).toHaveBeenCalledWith({ version: ready.version });
    });

    it("should install on a ready toolbar click even if a stale snapshot arrives after the event", async () => {
        let resolve!: (status: AppUpdateStatus) => void;
        const snapshot = vi.fn(() => new Promise<AppUpdateStatus>(done => { resolve = done; }));
        mockInvokeHandler("get_app_update_status", snapshot);
        render(<AppUpdateNotice />);
        await waitFor(() => expect(snapshot).toHaveBeenCalled());
        await act(() => emit("app-update-status", ready));
        await act(async () => resolve(downloading));
        expect(install).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: /点击安装更新/ }));
        expect(install).toHaveBeenCalledExactlyOnceWith({ version: ready.version });
        expect(screen.queryByRole("dialog")).toBeNull();
    });

    it("should show bytes for unknown totals and allow retry after a failed download", async () => {
        const retry = vi.fn();
        mockInvokeHandler("check_app_update", retry);
        mockInvokeHandler("get_app_update_status", () => ({ ...downloading, total: null }));
        render(<AppUpdateNotice />);
        fireEvent.click(await screen.findByRole("button", { name: /正在下载/ }));
        expect(screen.getByText("已下载 512 B")).toBeVisible();
        const progress = screen.getByRole("progressbar", { name: "下载进度" });
        expect(progress).not.toHaveAttribute("aria-valuenow");
        expect(progress).toHaveAttribute("data-state", "indeterminate");
        await act(() => emit("app-update-status", { ...downloading, revision: 3, phase: "failed", error: "连接中断" }));
        expect(screen.getByRole("alert")).toHaveTextContent("连接中断");
        fireEvent.click(screen.getByRole("button", { name: "使用代理检查并下载更新" }));
        expect(retry).toHaveBeenCalledWith({ useProxy: true });
        expect(install).not.toHaveBeenCalled();
    });
});
