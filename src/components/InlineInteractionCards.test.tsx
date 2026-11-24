import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { PreviewFileCard, type PreviewFileRequest } from "./InlineInteractionCards";
import { MessageTurnContext } from "./conversation/messageTurnContext";
import { clearMockHandler, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/components/UnifiedMarkdown", () => ({ default: () => null }));

it("should open a local HTML file using the WebView relay address", () => {
    // happy-dom has no native protocol handler. Use an inert URL for the converted address.
    vi.mocked(convertFileSrc).mockReturnValueOnce("about:blank");
    const file = { type: "html", title: "报告", url: "aipp-preview://localhost/report-token" };
    render(<PreviewFileCard request={{ request_id: "local-html", files: [file] }} isOpen />);
    expect(convertFileSrc).toHaveBeenCalledWith("report-token", "aipp-preview");
    expect(screen.getByTitle("报告")).toHaveAttribute("src", "about:blank");
});

it("should show prepared HTML when the request also contains its source URL", () => {
    const file = { type: "html", title: "报告", content: "<h1>报告</h1>", url: "https://example.com/" };
    render(<PreviewFileCard request={{ request_id: "inline-html", files: [file] }} isOpen />);
    expect(screen.getByTitle("报告")).toHaveAttribute("srcdoc", file.content);
    expect(screen.getByTitle("报告")).not.toHaveAttribute("src");
});

it("should explain the placeholder when an HTML resource has not loaded", () => {
    const file = { type: "html", title: "报告", url: "about:blank" };
    render(<PreviewFileCard request={{ request_id: "pending-html", files: [file] }} isOpen />);
    expect(screen.getByText("HTML 文件尚未加载。")).toBeInTheDocument();
    expect(screen.queryByTitle("报告")).toBeNull();
    expect(screen.queryByRole("button", { name: "窗口预览" })).toBeNull();
});

it.each(["tabs", "list", "grid"] as const)("should enlarge the selected HTML and keep its inline frame in %s mode", async (viewMode) => {
    const file = { type: "html", title: "设计稿", content: "<h1>插件市场</h1>" };
    const files = [{ type: "html", title: "首页", content: "<h1>首页</h1>" }, file];
    render(<PreviewFileCard request={{ request_id: "window", files, viewMode }} isOpen />);
    if (viewMode === "tabs") fireEvent.click(screen.getByRole("button", { name: file.title }));
    const inline = screen.getByTitle(file.title);
    const actions = screen.getAllByRole("button", { name: "窗口预览" });
    fireEvent.click(actions[actions.length - 1]);
    const dialog = screen.getByRole("dialog", { name: file.title });
    expect(within(dialog).getByTitle(file.title)).toHaveAttribute("srcdoc", file.content);
    fireEvent.click(within(dialog).getByRole("button", { name: "关闭" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByTitle(file.title)).toBe(inline);
});

it("should collapse and expand the preview body when the header toggle is clicked", () => {
    const request: PreviewFileRequest = {
        request_id: "collapse",
        files: [{ type: "image", title: "封面", url: "data:image/png;base64,AAAA" }],
    };
    render(<PreviewFileCard request={request} isOpen />);
    expect(screen.getByRole("img", { name: "封面" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "收起预览" }));
    expect(screen.queryByRole("img", { name: "封面" })).toBeNull();
    expect(screen.getByText("封面")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "展开预览" }));
    expect(screen.getByRole("img", { name: "封面" })).toBeInTheDocument();
});

it("should fold the preview once a newer turn starts, like preview_code", () => {
    const request: PreviewFileRequest = {
        request_id: "turn",
        files: [{ type: "image", title: "封面", url: "data:image/png;base64,AAAA" }],
    };
    const renderInTurn = (isLatest: boolean) => (
        <MessageTurnContext.Provider value={isLatest}>
            <PreviewFileCard request={request} isOpen />
        </MessageTurnContext.Provider>
    );
    const { rerender } = render(renderInTurn(true));
    expect(screen.getByRole("img", { name: "封面" })).toBeInTheDocument();

    rerender(renderInTurn(false));
    expect(screen.queryByRole("img", { name: "封面" })).toBeNull();
    expect(screen.getByRole("button", { name: "展开预览" })).toBeInTheDocument();

    // 旧一轮的预览仍可手动展开查看
    fireEvent.click(screen.getByRole("button", { name: "展开预览" }));
    expect(screen.getByRole("img", { name: "封面" })).toBeInTheDocument();
});

it("should start folded when mounted inside an earlier turn", () => {
    const request: PreviewFileRequest = {
        request_id: "history",
        files: [{ type: "image", title: "封面", url: "data:image/png;base64,AAAA" }],
    };
    render(
        <MessageTurnContext.Provider value={false}>
            <PreviewFileCard request={request} isOpen />
        </MessageTurnContext.Provider>,
    );
    expect(screen.queryByRole("img", { name: "封面" })).toBeNull();
    expect(screen.getByText("封面")).toBeInTheDocument();
});

it("should render the WebView relay image when the user allows a pending URL", async () => {
    const originalUrl = "https://example.com/image.png";
    const placeholder = "data:image/svg+xml,%3Csvg/%3E";
    const file = { type: "image", title: "生成的图片", url: placeholder };
    const request: PreviewFileRequest = {
        request_id: "preview",
        files: [file],
        externalResources: {
            requestId: "preview",
            resources: [{
                id: "token", originalUrl, normalizedUrl: originalUrl, type: "image",
                source: "preview_file", occurrence: "file url", status: "pending", risk: "low",
            }],
        },
    };
    // 按命令名应答：弹窗打开时还会读取网络代理配置。
    mockInvokeHandler("authorize_preview_external_resources", () => ({
        previewFile: { request_id: "preview", files: [{ ...file, url: "aipp-preview://localhost/token" }] },
    }));
    vi.mocked(convertFileSrc).mockImplementationOnce((token, protocol) => `http://${protocol}.localhost/${token}`);

    render(<PreviewFileCard request={request} isOpen onOpenChange={vi.fn()} />);
    const image = screen.getByRole("img", { name: file.title });
    expect(image).toHaveAttribute("src", placeholder);
    fireEvent.click(screen.getByRole("button", { name: "加载外部资源" }));
    fireEvent.click(screen.getByRole("button", { name: "允许本次加载" }));
    await waitFor(() => expect(image).toHaveAttribute("src", "http://aipp-preview.localhost/token"));
    clearMockHandler("authorize_preview_external_resources");
});
