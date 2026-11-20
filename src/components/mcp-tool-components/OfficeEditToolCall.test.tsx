import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => mockInvoke(...args), convertFileSrc: () => "" }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }));
const startSave = vi.fn();
vi.mock("@/components/office/useOfficeSaveFlow", () => ({ useOfficeSaveFlow: () => ({ startSave, dialog: null }) }));
const openModal = vi.fn();
vi.mock("@/utils/officeFile", async (importOriginal) => ({
    ...(await importOriginal<typeof import("@/utils/officeFile")>()),
    openOfficePreviewModal: (...args: unknown[]) => openModal(...args),
}));

import OfficeEditToolCall, { parseOfficeEditError, parseOfficeEditResult } from "./OfficeEditToolCall";

const RESULT = JSON.stringify([
    {
        type: "json",
        json: {
            status: "saved",
            output_ref: "ofr_out",
            file_name: "报告（AI 修改）.docx",
            format: "docx",
            size_bytes: 2048,
            source_file_name: "报告.docx",
        },
    },
]);

function call(status: string, extra: Record<string, unknown> = {}) {
    return { call_id: 1, conversation_id: 9, status, ...extra } as never;
}

describe("OfficeEditToolCall", () => {
    beforeEach(() => {
        mockInvoke.mockReset();
        startSave.mockReset();
        openModal.mockReset();
    });

    it("should parse stored results and tool errors", () => {
        expect(parseOfficeEditResult(RESULT)?.output_ref).toBe("ofr_out");
        expect(parseOfficeEditResult("not json")).toBeNull();
        expect(
            parseOfficeEditError('工具执行错误: [{"type":"text","text":"office_edit 失败（stale_reference）：「报告.docx」在用户选择之后已被修改"}]'),
        ).toEqual({ code: "stale_reference", message: "「报告.docx」在用户选择之后已被修改" });
    });

    it("should show progress while the edit runs", () => {
        render(<OfficeEditToolCall toolName="office_edit" status="executing" currentToolCall={call("executing")} />);
        expect(screen.getByTestId("office-edit-card")).toHaveAttribute("data-state", "running");
    });

    it("should show why no file was produced", () => {
        render(
            <OfficeEditToolCall
                toolName="office_edit"
                status="failed"
                currentToolCall={call("failed", { error: '工具执行错误: [{"type":"text","text":"office_edit 失败（ambiguous_target）：「旧」出现 2 次"}]' })}
            />,
        );
        const card = screen.getByTestId("office-edit-card");
        expect(card).toHaveAttribute("data-state", "failed");
        expect(card).toHaveTextContent("没有生成修改后的文件");
        expect(card).toHaveTextContent("「旧」出现 2 次（原文件未改动）");
    });

    it("should enable preview, open and save-as only after the file is confirmed on disk", async () => {
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_file_ref_status") {
                return { refId: "ofr_out", available: true, fileName: "报告（AI 修改）.docx", sizeBytes: 2048, problem: null };
            }
            return null;
        });
        render(<OfficeEditToolCall toolName="office_edit" status="success" conversationId={9} currentToolCall={call("success", { result: RESULT })} />);
        await waitFor(() => expect(screen.getByTestId("office-edit-card")).toHaveAttribute("data-state", "ready"));
        expect(screen.getByTestId("office-edit-card")).toHaveTextContent("已另存为新文件，「报告.docx」未改动");

        fireEvent.click(screen.getByText("预览"));
        expect(openModal).toHaveBeenCalledWith(expect.objectContaining({ source: { refId: "ofr_out" }, conversationId: 9 }));
        fireEvent.click(screen.getByText("用外部应用打开"));
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("open_office_file_externally", { source: { refId: "ofr_out" } }));
        fireEvent.click(screen.getByText("另存为"));
        expect(startSave).toHaveBeenCalledWith({ source: { refId: "ofr_out" }, fileName: "报告（AI 修改）.docx", format: "docx", purpose: "output" });
    });

    it("should mark a missing file as unavailable instead of offering broken actions", async () => {
        mockInvoke.mockResolvedValue({ refId: "ofr_out", available: false, problem: "文件已被删除或移动", fileName: "报告（AI 修改）.docx", sizeBytes: 2048 });
        render(<OfficeEditToolCall toolName="office_edit" status="success" currentToolCall={call("success", { result: RESULT })} />);
        await waitFor(() => expect(screen.getByTestId("office-edit-card")).toHaveAttribute("data-state", "unavailable"));
        expect(screen.getByTestId("office-edit-card")).toHaveTextContent("文件已被删除或移动");
        expect(screen.getByText("预览").closest("button")).toBeDisabled();
        expect(screen.getByText("另存为").closest("button")).toBeDisabled();
    });
});
