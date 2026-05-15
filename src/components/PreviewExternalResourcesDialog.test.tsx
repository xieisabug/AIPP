import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import PreviewExternalResourcesDialog from "@/components/PreviewExternalResourcesDialog";
import { PreviewExternalResourcesPayload } from "@/utils/previewExternalResources";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

const pendingResources: PreviewExternalResourcesPayload = {
    requestId: "request-1",
    resources: [
        {
            id: "image-ok",
            originalUrl: "https://example.com/ok.png",
            normalizedUrl: "https://example.com/ok.png",
            type: "image",
            source: "preview_code",
            occurrence: "img src",
            status: "pending",
            risk: "low",
        },
        {
            id: "image-failed",
            originalUrl: "https://example.com/failed.png",
            normalizedUrl: "https://example.com/failed.png",
            type: "image",
            source: "preview_code",
            occurrence: "img src",
            status: "pending",
            risk: "low",
        },
    ],
};

describe("PreviewExternalResourcesDialog", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("keeps failed resources retryable and hides successfully loaded resources", async () => {
        const onOpenChange = vi.fn();
        const onAuthorized = vi.fn();
        const onAuthorizeSelected = vi.fn(async () => ({
            previewCode: {
                externalResources: {
                    requestId: "request-1",
                    resources: [
                        {
                            ...pendingResources.resources[0],
                            status: "allowed" as const,
                            allowedBy: "user" as const,
                        },
                        {
                            ...pendingResources.resources[1],
                            status: "failed" as const,
                            reason: "proxy failed",
                        },
                    ],
                },
            },
        }));

        render(
            <PreviewExternalResourcesDialog
                externalResources={pendingResources}
                open
                onOpenChange={onOpenChange}
                onAuthorized={onAuthorized}
                onAuthorizeSelected={onAuthorizeSelected}
            />
        );

        const user = userEvent.setup();
        await user.click(screen.getByRole("button", { name: "允许本次加载" }));

        await waitFor(() => {
            expect(screen.getByText("部分资源加载失败，请检查代理或网络后重试。")).toBeInTheDocument();
        });
        expect(screen.queryByText("https://example.com/ok.png")).not.toBeInTheDocument();
        expect(screen.getByText("https://example.com/failed.png")).toBeInTheDocument();
        expect(screen.getByText("proxy failed")).toBeInTheDocument();
        expect(onOpenChange).not.toHaveBeenCalledWith(false);
    });

    it("sends explicit proxy authorization through the fallback command path", async () => {
        mockInvokeHandler("authorize_preview_external_resources", () => ({
            previewCode: {
                externalResources: {
                    requestId: "request-1",
                    resources: [
                        {
                            ...pendingResources.resources[0],
                            status: "allowed",
                            allowedBy: "user",
                        },
                    ],
                },
            },
        }));
        const onOpenChange = vi.fn();

        render(
            <PreviewExternalResourcesDialog
                externalResources={{
                    requestId: "request-1",
                    resources: [pendingResources.resources[0]],
                }}
                open
                onOpenChange={onOpenChange}
                onAuthorized={vi.fn()}
            />
        );

        const user = userEvent.setup();
        await user.click(screen.getByRole("button", { name: "使用代理加载所选资源" }));

        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith(
                "authorize_preview_external_resources",
                expect.objectContaining({
                    requestId: "request-1",
                    request_id: "request-1",
                    resourceIds: ["image-ok"],
                    resource_ids: ["image-ok"],
                    useProxy: true,
                    use_proxy: true,
                })
            );
        });
    });
});
