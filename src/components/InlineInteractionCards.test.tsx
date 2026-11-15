import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { expect, it, vi } from "vitest";
import { PreviewFileCard, type PreviewFileRequest } from "./InlineInteractionCards";

vi.mock("@/components/UnifiedMarkdown", () => ({ default: () => null }));

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
    fireEvent.click(screen.getByRole("button", { name: "需要加载外部资源" }));
    fireEvent.click(screen.getByRole("button", { name: "允许本次加载" }));
    await waitFor(() => expect(image).toHaveAttribute("src", "http://aipp-preview.localhost/token"));
});
