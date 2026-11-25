import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import React, { forwardRef, useEffect, useImperativeHandle } from "react";

const mockInvoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
    invoke: (...args: unknown[]) => mockInvoke(...args),
    convertFileSrc: () => "about:blank",
}));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }));

const startSave = vi.fn();
vi.mock("./useOfficeSaveFlow", () => ({
    useOfficeSaveFlow: () => ({ startSave, dialog: null }),
}));

/** 用假的预览面板驱动回调：选区、文件指纹、协议版本、保存。 */
const paneControl: {
    props: Record<string, any> | null;
    requestSave: ReturnType<typeof vi.fn>;
} = { props: null, requestSave: vi.fn() };
vi.mock("./OfficePreviewPane", () => {
    const Pane = forwardRef<unknown, Record<string, any>>((props, ref) => {
        paneControl.props = props;
        useImperativeHandle(ref, () => ({ requestSave: paneControl.requestSave }));
        useEffect(() => {
            props.onViewerProtocol?.(2);
            props.onFileLoaded?.({ fileName: "报告.docx", format: "docx", sha256: "sha-at-open", sizeBytes: 10 });
        }, []);
        return <div data-testid="fake-pane" data-mode={props.mode} data-require-edit={String(props.requireEdit)} />;
    });
    return { default: Pane, OfficeFormatIcon: () => null };
});

import OfficePreviewModal from "./OfficePreviewModal";
import { PreviewFileCard } from "../InlineInteractionCards";
vi.mock("@/components/UnifiedMarkdown", () => ({ default: () => null }));
import { openOfficePreviewModal } from "@/utils/officeFile";
import {
    __resetOfficeSelectionReceiversForTests,
    registerOfficeSelectionReceiver,
} from "@/services/officeSelectionReceiver";

const selection = { format: "docx", selectedText: "旧的说法", paragraphText: "这里是旧的说法。" };

function open(conversationId: number | null = 7) {
    act(() => {
        openOfficePreviewModal({ title: "报告.docx", source: { attachmentId: 3 }, conversationId });
    });
}

describe("OfficePreviewModal", () => {
    beforeEach(() => {
        mockInvoke.mockReset();
        startSave.mockReset();
        paneControl.requestSave = vi.fn();
        paneControl.props = null;
        __resetOfficeSelectionReceiversForTests();
    });

    const fileRef = { refId: "ofr_aaaaaaaaaaaaaaaa", fileName: "报告.docx", format: "docx" };

    function beginSuggestion(text: string) {
        act(() => paneControl.props!.onSelectionChange({ ...selection, selectedText: text }, { x: 130, y: 110 }));
    }

    function confirmNote(note: string) {
        fireEvent.change(screen.getByLabelText("调整意见"), { target: { value: note } });
        fireEvent.click(screen.getByRole("button", { name: "确定" }));
    }

    it("should freeze, edit and remove selections and send the remaining badges together without a general request", async () => {
        const send = vi.fn().mockResolvedValue(undefined);
        registerOfficeSelectionReceiver({ conversationId: 7, label: "当前对话", send });
        mockInvoke.mockResolvedValue(fileRef);
        render(<OfficePreviewModal />);
        open();
        expect(screen.getByTestId("office-ai-send")).toBeDisabled();
        beginSuggestion("第一处");
        act(() => paneControl.props!.onSelectionChange(null));
        confirmNote("精简");
        beginSuggestion("第二处");
        confirmNote("删除此处");
        beginSuggestion("第三处");
        confirmNote("补充日期");
        fireEvent.click(screen.getByTitle("删除建议 2"));
        fireEvent.click(screen.getByRole("button", { name: "建议 1" }));
        expect(screen.getByLabelText("调整意见")).toHaveValue("精简");
        confirmNote("精简到一句");
        beginSuggestion("取消的选区");
        fireEvent.click(screen.getByRole("button", { name: "取消" }));
        fireEvent.click(screen.getByTestId("office-ai-send"));
        await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
        expect(mockInvoke).toHaveBeenCalledExactlyOnceWith("register_office_selection_ref", {
            source: { attachmentId: 3 }, expectedSha256: "sha-at-open", conversationId: 7, includeDocumentText: false,
        });
        const prompt = send.mock.calls[0][0] as string;
        expect(prompt).toMatch(/建议 1：精简到一句[\s\S]*第一处[\s\S]*建议 3：补充日期[\s\S]*第三处/);
        expect(prompt).not.toMatch(/第二处|取消的选区/);
        await waitFor(() => expect(screen.queryByTestId("fake-pane")).toBeNull());
    });

    it.each([true, false])("should attach document text to a general request with suggestions=%s", async (addNote) => {
        const send = vi.fn();
        registerOfficeSelectionReceiver({ conversationId: 7, label: "当前对话", send });
        mockInvoke.mockResolvedValue({ ...fileRef, documentText: "原文件正文" });
        render(<OfficePreviewModal />);
        open();
        if (addNote) {
            beginSuggestion("某处");
            confirmNote("改用主动语态");
        }
        fireEvent.change(screen.getByLabelText("总意见"), { target: { value: "全文精简" } });
        fireEvent.click(screen.getByTestId("office-ai-send"));
        await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
        expect(mockInvoke.mock.calls[0][1].includeDocumentText).toBe(true);
        expect(send.mock.calls[0][0]).toMatch(/全文精简[\s\S]*原文件正文/);
        expect(send.mock.calls[0][0].includes("建议 1：改用主动语态")).toBe(addNote);
    });

    it("should offer AI edits when a Word file card opens a preview with a descriptive title", async () => {
        registerOfficeSelectionReceiver({ conversationId: 7, label: "总管家", send: vi.fn() });
        const file = { title: "分析报告", type: "docx", url: "aipp-preview://localhost/report-token" };
        render(<>
            <PreviewFileCard request={{ request_id: "report", conversation_id: 7, files: [file] }} isOpen />
            <OfficePreviewModal />
        </>);
        fireEvent.click(screen.getByRole("button", { name: "窗口预览" }));
        expect(await screen.findByTestId("office-mode-badge")).toHaveTextContent("只读预览");
        expect(paneControl.props?.extension).toBe("docx");
        expect(screen.getByTestId("office-enter-edit")).toBeVisible();
        expect(screen.getByTestId("office-ai-panel")).toHaveTextContent("总管家");
        act(() => paneControl.props!.onSelectionChange(selection));
        expect(screen.queryByLabelText("调整意见")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "给选区加建议" }));
        expect(screen.getByRole("dialog", { name: "建议 1" })).toHaveTextContent(selection.selectedText);
    });

    it("should refuse a stale selection and offer to reload instead of sending", async () => {
        const send = vi.fn();
        registerOfficeSelectionReceiver({ conversationId: 7, label: "当前对话", send });
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "register_office_selection_ref") throw "OFFICE_PREVIEW_ERROR:stale_selection:「报告.docx」在打开预览后已被修改";
            return null;
        });
        render(<OfficePreviewModal />);
        open(7);
        await screen.findByTestId("fake-pane");
        beginSuggestion("原文");
        confirmNote("改");
        fireEvent.click(screen.getByTestId("office-ai-send"));
        const error = await screen.findByTestId("office-ai-error");
        expect(error).toHaveTextContent("文件已被修改");
        expect(send).not.toHaveBeenCalled();
        fireEvent.click(screen.getByText("重新加载并清除旧选区"));
        expect(screen.queryByRole("button", { name: "建议 1" })).toBeNull();
        expect(screen.getByTestId("office-ai-send")).toBeDisabled();
    });

    it("should not offer AI edits when the preview belongs to a conversation that is not shown here", async () => {
        registerOfficeSelectionReceiver({ conversationId: 8, label: "别的对话", send: vi.fn() });
        render(<OfficePreviewModal />);
        open(7);
        await screen.findByTestId("fake-pane");
        act(() => paneControl.props!.onSelectionChange(selection));
        expect(screen.queryByTestId("office-ai-panel")).toBeNull();
        expect(screen.getByTestId("fake-pane")).toHaveAttribute("data-require-edit", "false");
    });

    it("should save manual edits as a copy and ask before discarding unsaved changes", async () => {
        const bytes = new Uint8Array([1]).buffer;
        paneControl.requestSave = vi.fn().mockResolvedValue(bytes);
        startSave.mockResolvedValue({ status: "saved", fileName: "报告（副本）.docx", location: "C:/x" });
        render(<OfficePreviewModal />);
        open(null);
        fireEvent.click(await screen.findByTestId("office-enter-edit"));
        expect(screen.getByTestId("fake-pane")).toHaveAttribute("data-mode", "edit");
        expect(screen.getByTestId("office-mode-badge")).toHaveTextContent("编辑中");

        act(() => paneControl.props!.onDirtyChange(true));
        fireEvent.click(screen.getByText("退出编辑"));
        expect(await screen.findByText("放弃未保存的修改？")).toBeInTheDocument();
        fireEvent.click(screen.getByText("继续编辑"));
        expect(screen.getByTestId("fake-pane")).toHaveAttribute("data-mode", "edit");

        fireEvent.click(screen.getByTestId("office-save-copy"));
        await waitFor(() => expect(startSave).toHaveBeenCalledTimes(1));
        expect(startSave).toHaveBeenCalledWith({
            source: { attachmentId: 3 },
            fileName: "报告.docx",
            format: "docx",
            purpose: "copy",
            bytes,
        });
        // 保存成功后不再提示未保存
        fireEvent.click(screen.getByText("退出编辑"));
        await waitFor(() => expect(screen.getByTestId("fake-pane")).toHaveAttribute("data-mode", "view"));
    });
});

// keep React in scope for the JSX in vi.mock factories
void React;
