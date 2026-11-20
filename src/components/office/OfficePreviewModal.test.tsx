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

    it("should send the request with a registered, fingerprint-checked selection to the conversation", async () => {
        const send = vi.fn().mockResolvedValue(undefined);
        registerOfficeSelectionReceiver({ conversationId: 7, label: "当前对话「周报」", send });
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "register_office_selection_ref") return { refId: "ofr_aaaaaaaaaaaaaaaa", fileName: "报告.docx", format: "docx" };
            return null;
        });
        render(<OfficePreviewModal />);
        open(7);
        await screen.findByTestId("fake-pane");
        expect(screen.getByTestId("fake-pane")).toHaveAttribute("data-require-edit", "true");

        act(() => paneControl.props!.onSelectionChange(selection));
        fireEvent.click(await screen.findByTestId("office-ask-ai"));
        // 发送前展示：目标文件、位置、交给助手的原文、发送到哪里
        expect(screen.getByTestId("office-ai-panel")).toHaveTextContent("报告.docx · Word 正文中选中的文字");
        expect(screen.getByTestId("office-ai-panel")).toHaveTextContent("发送到：当前对话「周报」");
        expect(screen.getByTestId("office-ai-target")).toHaveTextContent("旧的说法");
        expect(screen.getByTestId("office-ai-send")).toBeDisabled();

        fireEvent.change(screen.getByTestId("office-ai-request"), { target: { value: "改得更正式" } });
        fireEvent.click(screen.getByTestId("office-ai-send"));
        await waitFor(() => expect(send).toHaveBeenCalledTimes(1));
        expect(mockInvoke).toHaveBeenCalledWith("register_office_selection_ref", {
            source: { attachmentId: 3 },
            expectedSha256: "sha-at-open",
            conversationId: 7,
        });
        const prompt = send.mock.calls[0][0] as string;
        expect(prompt.startsWith("改得更正式\n\n<officeselection refid=\"ofr_aaaaaaaaaaaaaaaa\"")).toBe(true);
        await waitFor(() => expect(screen.queryByTestId("fake-pane")).toBeNull());
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
        act(() => paneControl.props!.onSelectionChange(selection));
        fireEvent.click(await screen.findByTestId("office-ask-ai"));
        fireEvent.change(screen.getByTestId("office-ai-request"), { target: { value: "改" } });
        fireEvent.click(screen.getByTestId("office-ai-send"));
        const error = await screen.findByTestId("office-ai-error");
        expect(error).toHaveTextContent("文件已被修改");
        expect(send).not.toHaveBeenCalled();
        fireEvent.click(screen.getByText("重新加载"));
        expect(screen.queryByTestId("office-ai-panel")).toBeNull();
    });

    it("should not offer AI edits when the preview belongs to a conversation that is not shown here", async () => {
        registerOfficeSelectionReceiver({ conversationId: 8, label: "别的对话", send: vi.fn() });
        render(<OfficePreviewModal />);
        open(7);
        await screen.findByTestId("fake-pane");
        act(() => paneControl.props!.onSelectionChange(selection));
        expect(screen.queryByTestId("office-ask-ai")).toBeNull();
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
