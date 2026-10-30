import { render, screen, waitFor } from "@testing-library/react";
import { useForm } from "react-hook-form";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { DataFolderConfigForm } from "./DataFolderConfigForm";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));

function Harness() {
    const form = useForm<any>({ defaultValues: { mode: "local", server_url: "", token: "" } });
    return <DataFolderConfigForm form={form as any} />;
}

describe("DataFolderConfigForm open-data-folder entry", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_sync_status", () => {
            throw new Error("no sync status in test");
        });
    });

    it("desktop shows 数据文件夹 / 打开", async () => {
        platformState.mobile = false;
        render(<Harness />);
        await waitFor(() => expect(screen.getByText("数据文件夹")).toBeTruthy());
        expect(screen.getByText("同步模式")).toBeTruthy();
    });

    it("mobile hides 数据文件夹 but keeps sync settings", async () => {
        platformState.mobile = true;
        render(<Harness />);
        await waitFor(() => expect(screen.getByText("同步模式")).toBeTruthy());
        expect(screen.queryByText("数据文件夹")).toBeNull();
    });
});
