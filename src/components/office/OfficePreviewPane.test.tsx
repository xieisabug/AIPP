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

import OfficePreviewPane from "./OfficePreviewPane";
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

function encodeOfficeResponse(fileName: string, bytes: Uint8Array): ArrayBuffer {
    const header = new TextEncoder().encode(
        JSON.stringify({ fileName, format: "docx", mime: "x", sizeBytes: bytes.length, sha256: "abc" }),
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
            expect(mockInvoke).toHaveBeenCalledWith("install_office_preview_plugin", {});
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

        await waitFor(() => expect(mockInvoke).toHaveBeenCalledWith("install_office_preview_plugin", {}));
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
});
