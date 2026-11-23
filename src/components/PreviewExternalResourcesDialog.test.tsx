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
            expect(screen.getByText("部分资源加载失败，原因见各项说明。")).toBeInTheDocument();
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

    it("should show the resolved address and the referencing stylesheet when a font comes from external CSS", () => {
        render(
            <PreviewExternalResourcesDialog
                externalResources={{
                    requestId: "request-2",
                    resources: [
                        {
                            id: "font-1",
                            originalUrl: "./files/a-400-normal.woff",
                            normalizedUrl: "https://cdn.jsdelivr.net/npm/@fontsource/a/files/a-400-normal.woff",
                            type: "font",
                            source: "preview_code",
                            occurrence: "css secondary",
                            status: "pending",
                            risk: "medium",
                            via: "https://cdn.jsdelivr.net/npm/@fontsource/a/index.css",
                        },
                    ],
                }}
                open
                onOpenChange={vi.fn()}
                onAuthorized={vi.fn()}
            />
        );
        const url = screen.getByText("https://cdn.jsdelivr.net/npm/@fontsource/a/files/a-400-normal.woff");
        expect(url).toHaveAttribute("title", "原写法：./files/a-400-normal.woff");
        expect(screen.getByText("字体")).toBeInTheDocument();
        expect(screen.getByText("cdn.jsdelivr.net")).toBeInTheDocument();
        expect(screen.getByText("由样式表 cdn.jsdelivr.net/npm/@fontsource/a/index.css 引用")).toBeInTheDocument();
        expect(screen.queryByText(/preview_code|css secondary/)).not.toBeInTheDocument();
    });

    it("should list blocked addresses without selecting them and close once only those remain", async () => {
        const blockedResource = {
            id: "blocked-1",
            originalUrl: "./logo.png",
            normalizedUrl: "./logo.png",
            type: "image" as const,
            source: "preview_code",
            occurrence: "<img> src",
            status: "blocked" as const,
            risk: "low" as const,
            reason: "Unsupported preview resource URL",
        };
        const onOpenChange = vi.fn();
        const onAuthorizeSelected = vi.fn(async () => ({
            previewCode: {
                externalResources: {
                    requestId: "request-1",
                    resources: [
                        { ...pendingResources.resources[0], status: "allowed" as const, allowedBy: "user" as const },
                        blockedResource,
                    ],
                },
            },
        }));
        render(
            <PreviewExternalResourcesDialog
                externalResources={{ requestId: "request-1", resources: [pendingResources.resources[0], blockedResource] }}
                open
                onOpenChange={onOpenChange}
                onAuthorized={vi.fn()}
                onAuthorizeSelected={onAuthorizeSelected}
            />
        );
        expect(screen.getByRole("region", { name: "无法加载的地址" })).toHaveTextContent("./logo.png");
        expect(screen.getAllByRole("checkbox")).toHaveLength(1);
        const user = userEvent.setup();
        await user.click(screen.getByRole("button", { name: "允许本次加载" }));
        await waitFor(() => expect(onOpenChange).toHaveBeenCalledWith(false));
        expect(onAuthorizeSelected).toHaveBeenCalledWith(["image-ok"], { addToWhitelist: false, useProxy: false });
    });

    it("should disable proxy loading and point to network settings when no proxy is configured", async () => {
        mockInvokeHandler("get_all_feature_config", () => [
            { feature_code: "network_config", key: "network_proxy", value: "  " },
        ]);
        render(
            <PreviewExternalResourcesDialog
                externalResources={{ requestId: "request-1", resources: [pendingResources.resources[0]] }}
                open
                onOpenChange={vi.fn()}
                onAuthorized={vi.fn()}
            />
        );
        expect(await screen.findByText("未配置网络代理（设置 → 网络配置）")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "使用代理加载所选资源" })).toBeDisabled();
        expect(screen.getByRole("button", { name: "允许本次加载" })).toBeEnabled();
    });
});
