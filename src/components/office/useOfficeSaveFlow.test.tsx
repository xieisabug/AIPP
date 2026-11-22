import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";

const mockInvoke = vi.fn();
const mockSaveDialog = vi.fn();
const mockMobileAction = vi.fn();
let android = false;

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mockInvoke(...args), convertFileSrc: () => "" }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: (...args: unknown[]) => mockSaveDialog(...args) }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openPath: vi.fn().mockResolvedValue(undefined) }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }));
vi.mock("@/services/mobileFileService", () => ({
    isAndroid: () => Promise.resolve(android),
    mobileFileAction: (...args: unknown[]) => mockMobileAction(...args),
    exportMime: () => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
}));

import { toast } from "sonner";
import { defaultCopyName, useOfficeSaveFlow, type OfficeSaveOutcome } from "./useOfficeSaveFlow";
import type { OfficeSaveRequest } from "@/services/officeSave";

let outcome: Promise<OfficeSaveOutcome> | null = null;

function Harness({ request }: { request: OfficeSaveRequest }) {
    const { startSave, dialog } = useOfficeSaveFlow();
    return (
        <>
            <button type="button" onClick={() => { outcome = startSave(request); }}>start</button>
            {dialog}
        </>
    );
}

const copyRequest: OfficeSaveRequest = {
    source: { attachmentId: 1 },
    fileName: "报告.docx",
    format: "docx",
    purpose: "copy",
    bytes: new Uint8Array([80, 75, 3, 4]).buffer,
};

describe("useOfficeSaveFlow", () => {
    beforeEach(() => {
        mockInvoke.mockReset();
        mockSaveDialog.mockReset();
        mockMobileAction.mockReset();
        vi.mocked(toast.success).mockClear();
        android = false;
        outcome = null;
    });

    it("should name copies like the backend does", () => {
        expect(defaultCopyName("报告.docx", "copy")).toBe("报告（副本）.docx");
        expect(defaultCopyName("报告（AI 修改）.docx", "output")).toBe("报告（AI 修改）.docx");
    });

    function writeCopyHeader(args: unknown) {
        const body = args as Uint8Array;
        const headerLen = new DataView(body.buffer, body.byteOffset).getUint32(0, true);
        return JSON.parse(new TextDecoder().decode(body.subarray(4, 4 + headerLen)));
    }

    it("should keep the edited bytes and let the user re-pick when the destination is the source", async () => {
        const sourcePath = "C:/docs/报告.docx";
        const copyPath = "C:/docs/报告（副本）.docx";
        mockSaveDialog.mockResolvedValueOnce(sourcePath).mockResolvedValueOnce(copyPath);
        mockInvoke.mockImplementation(async (cmd: string, args: unknown) => {
            if (cmd === "suggest_office_copy_path") return copyPath;
            if (cmd === "office_write_copy") {
                const header = writeCopyHeader(args);
                expect(header.source).toEqual(copyRequest.source);
                expect(header.overwrite).toBe(false);
                if (header.targetPath === sourcePath) {
                    throw "OFFICE_PREVIEW_ERROR:protected_source:所选位置是原文件，请重新选择位置";
                }
                expect(Array.from((args as Uint8Array).slice(-4))).toEqual([80, 75, 3, 4]);
                return { path: copyPath, fileName: "报告（副本）.docx", sizeBytes: 4, format: "docx" };
            }
            return null;
        });

        render(<Harness request={copyRequest} />);
        fireEvent.click(screen.getByText("start"));
        fireEvent.click(await screen.findByTestId("office-save-choose"));
        expect(await screen.findByText("不能覆盖原文件")).toBeInTheDocument();
        expect(screen.queryByTestId("office-save-overwrite")).not.toBeInTheDocument();
        expect(toast.success).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "重新选择位置" }));
        await expect(outcome).resolves.toEqual({ status: "saved", fileName: "报告（副本）.docx", location: copyPath });
        expect(toast.success).toHaveBeenCalledTimes(1);
    });

    it("should show the format notice, then require a second confirmation before overwriting", async () => {
        mockInvoke.mockImplementation(async (cmd: string, args: unknown) => {
            if (cmd === "suggest_office_copy_path") return "C:/docs/报告（副本）.docx";
            if (cmd === "office_write_copy") {
                const header = writeCopyHeader(args);
                expect(header.source).toEqual(copyRequest.source);
                if (!header.overwrite) throw "OFFICE_PREVIEW_ERROR:target_exists:「报告.docx」已存在，未覆盖";
                return { path: header.targetPath, fileName: "报告.docx", sizeBytes: 4, format: "docx" };
            }
            return null;
        });
        mockSaveDialog.mockResolvedValue("C:/other/报告.docx");
        render(<Harness request={copyRequest} />);
        fireEvent.click(screen.getByText("start"));
        expect(await screen.findByText("另存为新文件")).toBeInTheDocument();
        expect(screen.getByTestId("office-save-dialog")).toHaveTextContent("原文件「报告.docx」保持不变");
        expect(screen.getByTestId("office-save-dialog")).toHaveTextContent("保存时保留原字体名");

        fireEvent.click(screen.getByTestId("office-save-choose"));
        await waitFor(() => expect(mockSaveDialog).toHaveBeenCalledWith({
            defaultPath: "C:/docs/报告（副本）.docx",
            filters: [{ name: "Word 文档", extensions: ["docx"] }],
        }));
        expect(await screen.findByText("覆盖「报告.docx」？")).toBeInTheDocument();
        const firstWrites = mockInvoke.mock.calls.filter(([c]) => c === "office_write_copy").length;
        expect(firstWrites).toBe(1);

        fireEvent.click(screen.getByTestId("office-save-overwrite"));
        await expect(outcome).resolves.toEqual({ status: "saved", fileName: "报告.docx", location: "C:/other/报告.docx" });
    });

    it("should report a write failure without claiming success", async () => {
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "suggest_office_copy_path") return "C:/docs/x.docx";
            if (cmd === "office_write_copy") throw "OFFICE_PREVIEW_ERROR:disk_full:写入失败：磁盘空间不足";
            return null;
        });
        mockSaveDialog.mockResolvedValue("C:/docs/x.docx");
        render(<Harness request={copyRequest} />);
        fireEvent.click(screen.getByText("start"));
        fireEvent.click(await screen.findByTestId("office-save-choose"));
        expect(await screen.findByText("磁盘空间不足")).toBeInTheDocument();
        expect(screen.getByTestId("office-save-dialog")).toHaveTextContent("原文件未受影响");
        fireEvent.click(screen.getByText("关闭"));
        await expect(outcome).resolves.toEqual({ status: "cancelled" });
    });

    it("should stay on the notice when the system dialog is cancelled, and resolve cancelled on close", async () => {
        mockInvoke.mockResolvedValue("C:/docs/x.docx");
        mockSaveDialog.mockResolvedValue(null);
        render(<Harness request={copyRequest} />);
        fireEvent.click(screen.getByText("start"));
        fireEvent.click(await screen.findByTestId("office-save-choose"));
        await waitFor(() => expect(mockSaveDialog).toHaveBeenCalled());
        expect(screen.getByText("另存为新文件")).toBeInTheDocument();
        expect(mockInvoke.mock.calls.some(([c]) => c === "office_write_copy")).toBe(false);
        fireEvent.click(screen.getByText("取消"));
        await expect(outcome).resolves.toEqual({ status: "cancelled" });
    });

    it("should save through the Android document picker and only count status saved", async () => {
        android = true;
        mockMobileAction.mockResolvedValue({ status: "saved", uri: "content://docs/1" });
        render(<Harness request={copyRequest} />);
        fireEvent.click(screen.getByText("start"));
        const choose = await screen.findByTestId("office-save-choose");
        await waitFor(() => expect(choose).toHaveTextContent("保存到手机"));
        await act(async () => {
            fireEvent.click(choose);
        });
        await expect(outcome).resolves.toEqual({ status: "saved", fileName: "报告（副本）.docx", location: "content://docs/1" });
        expect(mockMobileAction).toHaveBeenCalledWith("save", expect.objectContaining({ name: "报告（副本）.docx" }));
        expect(mockSaveDialog).not.toHaveBeenCalled();
    });
});
