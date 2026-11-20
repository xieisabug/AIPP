import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { PreviewFileCard, type PreviewFileRequest } from "./InlineInteractionCards";
import { MessageTurnContext } from "./conversation/messageTurnContext";

vi.mock("@/components/UnifiedMarkdown", () => ({ default: () => null }));

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
    vi.mocked(invoke).mockResolvedValueOnce({
        previewFile: { request_id: "preview", files: [{ ...file, url: "aipp-preview://localhost/token" }] },
    });
    vi.mocked(convertFileSrc).mockImplementationOnce((token, protocol) => `http://${protocol}.localhost/${token}`);

    render(<PreviewFileCard request={request} isOpen onOpenChange={vi.fn()} />);
    const image = screen.getByRole("img", { name: file.title });
    expect(image).toHaveAttribute("src", placeholder);
    fireEvent.click(screen.getByRole("button", { name: "加载外部资源" }));
    fireEvent.click(screen.getByRole("button", { name: "允许本次加载" }));
    await waitFor(() => expect(image).toHaveAttribute("src", "http://aipp-preview.localhost/token"));
});
