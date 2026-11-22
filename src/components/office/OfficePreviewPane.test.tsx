import { describe, expect, it, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";

const mockInvoke = vi.fn();
const eventListeners = new Map<string, Array<(event: { payload: unknown }) => void>>();

vi.mock("@tauri-apps/api/core", () => ({
    invoke: (...args: unknown[]) => mockInvoke(...args),
    // about:blank keeps happy-dom from fetching the viewer URL
    convertFileSrc: () => "about:blank",
}));

vi.mock("@tauri-apps/api/event", () => ({
    listen: vi.fn((event: string, cb: (event: { payload: unknown }) => void) => {
        const list = eventListeners.get(event) ?? [];
        list.push(cb);
        eventListeners.set(event, list);
        return Promise.resolve(() => {
            eventListeners.set(
                event,
                (eventListeners.get(event) ?? []).filter((item) => item !== cb),
            );
        });
    }),
}));

vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => false,
}));

import React from "react";
import OfficePreviewPane, { type OfficePreviewPaneHandle } from "./OfficePreviewPane";
import { __resetOfficePreviewPluginStateForTests } from "@/services/officePreviewPlugin";

const READY_STATUS = {
    pluginCode: "office-preview-plugin",
    installed: true,
    enabled: true,
    viewerReady: true,
    version: "0.1.0",
    localDevAvailable: false,
};

const MISSING_STATUS = {
    pluginCode: "office-preview-plugin",
    installed: false,
    enabled: false,
    viewerReady: false,
    localDevAvailable: false,
};

function emit(event: string, payload: unknown) {
    for (const fn of eventListeners.get(event) ?? []) fn({ payload });
}

function encodeOfficeResponse(fileName: string, bytes: Uint8Array, copyOriginToken?: string): ArrayBuffer {
    const header = new TextEncoder().encode(
        JSON.stringify({ fileName, format: "docx", mime: "x", sizeBytes: bytes.length, sha256: "abc", copyOriginToken }),
    );
    const out = new Uint8Array(4 + header.length + bytes.length);
    new DataView(out.buffer).setUint32(0, header.length, true);
    out.set(header, 4);
    out.set(bytes, 4 + header.length);
    return out.buffer;
}

describe("OfficePreviewPane", () => {
    beforeEach(() => {
        mockInvoke.mockReset();
        eventListeners.clear();
        __resetOfficePreviewPluginStateForTests();
    });

    it("should auto download the plugin with live progress and then load the file when plugin is missing", async () => {
        let finishInstall: (value: unknown) => void = () => {};
        mockInvoke.mockImplementation((cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return Promise.resolve(MISSING_STATUS);
            if (cmd === "install_office_preview_plugin") {
                return new Promise((resolve) => {
                    finishInstall = resolve;
                });
            }
            if (cmd === "read_authorized_office_file") {
                return Promise.resolve(encodeOfficeResponse("季度报告.docx", new Uint8Array([80, 75, 3, 4])));
            }
            return Promise.resolve(null);
        });

        render(<OfficePreviewPane title="季度报告.docx" source={{ attachmentId: 101 }} />);

        await waitFor(() => {
            expect(mockInvoke).toHaveBeenCalledWith("install_office_preview_plugin", { requireEdit: false });
        });
        expect(screen.getByTestId("office-plugin-install-progress")).toBeInTheDocument();

        act(() => {
            emit("plugin_install_progress", {
                pluginCode: "office-preview-plugin",
                stage: "downloading",
                downloadedBytes: 21 * 1024 * 1024,
                totalBytes: 42 * 1024 * 1024,
                percent: 50,
                message: "正在下载插件包 (21.0 / 42.0 MiB)...",
            });
        });
        expect(screen.getByText("正在下载插件包 (21.0 / 42.0 MiB)...")).toBeInTheDocument();
        expect(screen.getByText("21.0 MiB / 42.0 MiB")).toBeInTheDocument();

        await act(async () => {
            finishInstall(READY_STATUS);
        });

        await waitFor(() => {
            expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", {
                source: { attachmentId: 101 },
            });
        });
        expect(screen.getByTitle("季度报告.docx").tagName).toBe("IFRAME");
    });

    it("should share one install between panes opened at the same time", async () => {
        mockInvoke.mockImplementation((cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return Promise.resolve(MISSING_STATUS);
            if (cmd === "install_office_preview_plugin") return new Promise(() => {});
            return Promise.resolve(null);
        });

        render(
            <>
                <OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} />
                <OfficePreviewPane title="b.xlsx" source={{ attachmentId: 2 }} />
            </>,
        );

        await waitFor(() => {
            expect(screen.getAllByTestId("office-plugin-install-progress")).toHaveLength(2);
        });
        expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "install_office_preview_plugin")).toHaveLength(1);
    });

    it("should show the install error with a retry button when the download fails", async () => {
        mockInvoke.mockImplementation((cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return Promise.resolve(MISSING_STATUS);
            if (cmd === "install_office_preview_plugin") return Promise.reject("下载插件压缩包失败：timeout");
            return Promise.resolve(null);
        });

        render(<OfficePreviewPane title="a.pptx" source={{ attachmentId: 1 }} />);

        expect(await screen.findByTestId("office-plugin-install-error")).toHaveTextContent("timeout");
        expect(screen.getByText("重新下载")).toBeInTheDocument();
        expect(screen.getByText("用外部应用打开")).toBeInTheDocument();
    });

    it("should auto update an outdated plugin even when it is installed and enabled", async () => {
        mockInvoke.mockImplementation((cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") {
                return Promise.resolve({ ...READY_STATUS, version: "0.1.0", updateRequired: true });
            }
            if (cmd === "install_office_preview_plugin") return new Promise(() => {});
            return Promise.resolve(null);
        });

        render(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} />);

        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("install_office_preview_plugin", { requireEdit: false }));
        expect(screen.getByText("正在更新 Office 预览组件")).toBeInTheDocument();
        expect(mockInvoke).not.toHaveBeenCalledWith("read_authorized_office_file", expect.anything());
    });

    it("should not auto install when the plugin was disabled by the user", async () => {
        mockInvoke.mockImplementation((cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") {
                return Promise.resolve({ ...READY_STATUS, enabled: false });
            }
            if (cmd === "enable_office_preview_plugin") return Promise.resolve(READY_STATUS);
            return Promise.resolve(null);
        });

        render(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} />);

        expect(await screen.findByTestId("office-plugin-disabled")).toBeInTheDocument();
        expect(mockInvoke).not.toHaveBeenCalledWith("install_office_preview_plugin", expect.anything());

        fireEvent.click(screen.getByText("启用并预览"));
        await waitFor(() => {
            expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", { source: { attachmentId: 1 } });
        });
    });

    it("should display structured error state and support external app fallback when file is encrypted", async () => {
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return READY_STATUS;
            if (cmd === "read_authorized_office_file") {
                throw "OFFICE_PREVIEW_ERROR:password_encrypted:该文档已设置密码加密，暂不支持应用内预览";
            }
            return null;
        });

        render(<OfficePreviewPane title="机密财报.xlsx" source={{ attachmentId: 202 }} />);

        const errBox = await screen.findByTestId("office-preview-error");
        expect(errBox).toHaveAttribute("data-error-code", "password_encrypted");
        expect(screen.getByText("文件受密码加密保护")).toBeInTheDocument();

        fireEvent.click(screen.getByText("用外部应用打开"));
        await waitFor(() => {
            expect(mockInvoke).toHaveBeenCalledWith("open_office_file_externally", {
                source: { attachmentId: 202 },
            });
        });

        fireEvent.click(screen.getByText("重试"));
        await waitFor(() => {
            expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "read_authorized_office_file")).toHaveLength(2);
        });
    });

    function mockReadyWithFile() {
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return READY_STATUS;
            if (cmd === "read_authorized_office_file") {
                return encodeOfficeResponse("a.docx", new Uint8Array([80, 75, 3, 4]));
            }
            return null;
        });
    }

    /** 模拟查看器 iframe 发给宿主的消息，并拦截宿主发给查看器的消息。 */
    async function mountViewer() {
        const iframe = (await screen.findByTitle("a.docx")) as HTMLIFrameElement;
        const frameWindow = iframe.contentWindow as Window;
        const posted = vi.fn();
        frameWindow.postMessage = posted as unknown as Window["postMessage"];
        const fromViewer = (data: Record<string, unknown>) =>
            act(() => {
                window.dispatchEvent(new MessageEvent("message", { data, source: frameWindow }));
            });
        return { posted, fromViewer };
    }

    it("should bind an editable relay source without sending its save handle to the iframe", async () => {
        const loaded = vi.fn();
        const source = { relayToken: "preview-token" };
        mockInvoke.mockImplementation(async (cmd: string, args: { bindCopySource?: boolean }) => {
            if (cmd === "get_office_preview_plugin_status") return { ...READY_STATUS, editSupported: true };
            if (cmd === "read_authorized_office_file") {
                return encodeOfficeResponse("a.docx", new Uint8Array([80, 75, 3, 4]), args.bindCopySource ? "copy-token" : undefined);
            }
            return null;
        });
        const ui = render(<OfficePreviewPane title="a.docx" source={source} onFileLoaded={loaded} />);
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", { source }));
        ui.rerender(<OfficePreviewPane title="a.docx" source={source} mode="edit" onFileLoaded={loaded} />);
        await waitFor(() => expect(loaded).toHaveBeenLastCalledWith(expect.objectContaining({ copyOriginToken: "copy-token" })));
        expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", { source, bindCopySource: true });
        const { posted, fromViewer } = await mountViewer();
        await fromViewer({ type: "aipp-office:viewer-ready" });
        const load = posted.mock.calls.find(([msg]) => msg.type === "aipp-office:load")?.[0];
        expect(load).not.toHaveProperty("copyOriginToken");
        ui.unmount();
        expect(mockInvoke).toHaveBeenCalledWith("release_office_copy_origin", { token: "copy-token" });
    });

    it("should release an edit binding from a read that completed after the pane closed", async () => {
        let finishRead: (data: ArrayBuffer) => void = () => {};
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return { ...READY_STATUS, editSupported: true };
            if (cmd === "read_authorized_office_file") return new Promise<ArrayBuffer>(resolve => { finishRead = resolve; });
            return null;
        });
        const source = { relayToken: "preview-token" };
        const ui = render(<OfficePreviewPane title="a.docx" source={source} mode="edit" />);
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", { source, bindCopySource: true }));
        ui.unmount();
        await act(async () => finishRead(encodeOfficeResponse("a.docx", new Uint8Array([80, 75, 3, 4]), "late-token")));
        expect(mockInvoke).toHaveBeenCalledWith("release_office_copy_origin", { token: "late-token" });
    });

    it("should keep showing the document when the watchdog deadline passes after a successful render", async () => {
        mockReadyWithFile();
        render(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} />);
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", expect.anything()));
        const { posted, fromViewer } = await mountViewer();

        vi.useFakeTimers();
        try {
            await fromViewer({ type: "aipp-office:viewer-ready" });
            const load = posted.mock.calls.find(([msg]) => msg.type === "aipp-office:load")?.[0];
            expect(load).toBeTruthy();
            await fromViewer({ type: "aipp-office:loaded", requestId: load.requestId, format: "docx" });
            expect(screen.queryByTestId("office-preview-loading")).toBeNull();

            act(() => {
                vi.advanceTimersByTime(60_000);
            });
            expect(screen.queryByTestId("office-preview-error")).toBeNull();
            expect(screen.getByTitle("a.docx").tagName).toBe("IFRAME");
        } finally {
            vi.useRealTimers();
        }
    });

    it("should report a render timeout only when the viewer never answers and resend after retry", async () => {
        mockReadyWithFile();
        render(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} />);
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", expect.anything()));
        const first = await mountViewer();

        vi.useFakeTimers();
        try {
            await first.fromViewer({ type: "aipp-office:viewer-ready" });
            act(() => {
                vi.advanceTimersByTime(46_000);
            });
            expect(screen.getByTestId("office-preview-error")).toHaveAttribute("data-error-code", "render_timeout");
        } finally {
            vi.useRealTimers();
        }

        fireEvent.click(screen.getByText("重试"));
        const second = await mountViewer();
        // 新 iframe 尚未就绪前不能发送文档（否则消息会丢失）
        await waitFor(() => expect(mockInvoke.mock.calls.filter(([c]) => c === "read_authorized_office_file")).toHaveLength(2));
        expect(second.posted.mock.calls.some(([msg]) => msg.type === "aipp-office:load")).toBe(false);
        await second.fromViewer({ type: "aipp-office:viewer-ready" });
        await waitFor(() =>
            expect(second.posted.mock.calls.some(([msg]) => msg.type === "aipp-office:load")).toBe(true),
        );
    });

    it("should reject legacy .doc/.xls/.ppt immediately without downloading the plugin", async () => {
        mockInvoke.mockResolvedValue(MISSING_STATUS);

        render(<OfficePreviewPane title="旧版合同.doc" source={{ attachmentId: 303 }} />);

        const errBox = await screen.findByTestId("office-preview-error");
        expect(errBox).toHaveAttribute("data-error-code", "unsupported_legacy_format");
        expect(screen.getByText("暂不支持旧版二进制 Office 格式")).toBeInTheDocument();
        expect(mockInvoke).not.toHaveBeenCalledWith("install_office_preview_plugin", expect.anything());
        expect(mockInvoke).not.toHaveBeenCalledWith("read_authorized_office_file", expect.anything());
    });

    it("should forward viewer selections, file fingerprint and protocol to the host callbacks", async () => {
        mockReadyWithFile();
        const onSelectionChange = vi.fn();
        const onFileLoaded = vi.fn();
        const onViewerProtocol = vi.fn();
        render(
            <OfficePreviewPane
                title="a.docx"
                source={{ attachmentId: 1 }}
                onSelectionChange={onSelectionChange}
                onFileLoaded={onFileLoaded}
                onViewerProtocol={onViewerProtocol}
            />,
        );
        await waitFor(() => expect(onFileLoaded).toHaveBeenCalledWith(expect.objectContaining({ sha256: "abc", format: "docx" })));
        const { posted, fromViewer } = await mountViewer();
        await fromViewer({ type: "aipp-office:viewer-ready", protocol: 2 });
        expect(onViewerProtocol).toHaveBeenCalledWith(2);
        const load = await waitFor(() => {
            const msg = posted.mock.calls.find(([m]) => m.type === "aipp-office:load")?.[0];
            expect(msg).toBeTruthy();
            return msg;
        });
        expect(load.mode).toBe("view");

        const selection = { format: "docx", selectedText: "旧文字", paragraphText: "这里是旧文字。" };
        await fromViewer({ type: "aipp-office:selection", requestId: load.requestId, selection });
        expect(onSelectionChange).toHaveBeenLastCalledWith(selection);
        // 过期请求与格式不对的选区都不转发
        await fromViewer({ type: "aipp-office:selection", requestId: load.requestId + 99, selection });
        await fromViewer({ type: "aipp-office:selection", requestId: load.requestId, selection: { format: "docx" } });
        expect(onSelectionChange).toHaveBeenLastCalledWith(null);
    });

    it("should load in edit mode and resolve requestSave with the bytes the viewer returns", async () => {
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return { ...READY_STATUS, version: "0.2.0", editSupported: true };
            if (cmd === "read_authorized_office_file") return encodeOfficeResponse("a.docx", new Uint8Array([80, 75, 3, 4]));
            return null;
        });
        const paneRef = React.createRef<OfficePreviewPaneHandle>();
        const onDirtyChange = vi.fn();
        render(<OfficePreviewPane ref={paneRef} title="a.docx" source={{ attachmentId: 1 }} mode="edit" onDirtyChange={onDirtyChange} />);
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("read_authorized_office_file", expect.anything()));
        const { posted, fromViewer } = await mountViewer();
        await fromViewer({ type: "aipp-office:viewer-ready", protocol: 2 });
        const load = await waitFor(() => {
            const msg = posted.mock.calls.find(([m]) => m.type === "aipp-office:load")?.[0];
            expect(msg).toBeTruthy();
            return msg;
        });
        expect(load.mode).toBe("edit");
        await fromViewer({ type: "aipp-office:loaded", requestId: load.requestId, format: "docx" });
        await fromViewer({ type: "aipp-office:dirty", requestId: load.requestId });
        expect(onDirtyChange).toHaveBeenLastCalledWith(true);

        const saved = paneRef.current!.requestSave();
        const request = posted.mock.calls.find(([m]) => m.type === "aipp-office:save-request")?.[0];
        expect(request.requestId).toBe(load.requestId);
        const bytes = new Uint8Array([1, 2, 3]).buffer;
        await fromViewer({ type: "aipp-office:saved", requestId: request.requestId, bytes });
        await expect(saved).resolves.toBe(bytes);
    });

    it("should not resend the already transferred bytes when switching to edit mode", async () => {
        let reads = 0;
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return { ...READY_STATUS, version: "0.2.0", editSupported: true };
            if (cmd === "read_authorized_office_file") {
                reads += 1;
                return encodeOfficeResponse("a.docx", new Uint8Array([80, 75, 3, 4]));
            }
            return null;
        });
        const view = render(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} mode="view" />);
        await waitFor(() => expect(reads).toBe(1));
        const { posted, fromViewer } = await mountViewer();
        // 模拟真实 postMessage 的转移语义：已转移（长度为 0）的缓冲区再次转移会抛 DataCloneError
        posted.mockImplementation((msg: { bytes?: ArrayBuffer }) => {
            if (msg.bytes && msg.bytes.byteLength === 0) throw new DOMException("ArrayBuffer at index 0 is already detached.", "DataCloneError");
            if (msg.bytes) Object.defineProperty(msg.bytes, "byteLength", { value: 0 });
        });
        await fromViewer({ type: "aipp-office:viewer-ready", protocol: 2 });
        await waitFor(() => expect(posted.mock.calls.filter(([m]) => m.type === "aipp-office:load")).toHaveLength(1));

        view.rerender(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} mode="edit" />);
        await waitFor(() => expect(reads).toBe(2));
        await waitFor(() => {
            const loads = posted.mock.calls.filter(([m]) => m.type === "aipp-office:load").map(([m]) => m);
            expect(loads).toHaveLength(2);
            expect(loads[1].mode).toBe("edit");
        });
        expect(posted.mock.results.every((r) => r.type === "return")).toBe(true);
    });

    it("should update an old component before editing instead of loading a viewer without edit support", async () => {
        mockInvoke.mockImplementation(async (cmd: string) => {
            if (cmd === "get_office_preview_plugin_status") return { ...READY_STATUS, version: "0.1.1", editSupported: false };
            if (cmd === "install_office_preview_plugin") return new Promise(() => {});
            return null;
        });
        render(<OfficePreviewPane title="a.docx" source={{ attachmentId: 1 }} requireEdit />);
        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("install_office_preview_plugin", { requireEdit: true }));
        expect(screen.getByText("正在更新 Office 组件以支持编辑与 AI 修改")).toBeInTheDocument();
        expect(mockInvoke).not.toHaveBeenCalledWith("read_authorized_office_file", expect.anything());
    });
});
