import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";
import { PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX } from "@/utils/previewCode";

describe("InlineCodePreviewCard", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("renders final HTML content inside the host container", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "html_preview",
                    renderer: "html",
                    code: "<div>Rendered HTML</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={1}
                messageId={10}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        expect(await screen.findByText("html_preview")).toBeInTheDocument();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Rendered HTML")
        );
    });

    it("keeps generated styles inside a shadow root", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "scoped_styles",
                    renderer: "html",
                    code: "<style>body { color: rgb(255, 0, 0); }</style><div>Scoped UI</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={3}
                messageId={12}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Scoped UI")
        );
        const shadowStyles = Array.from(host.shadowRoot?.querySelectorAll("style") ?? []).map(
            (style) => style.textContent ?? ""
        );
        expect(shadowStyles.some((text) => text.includes(":host"))).toBe(true);
        expect(host.querySelector("style")).toBeNull();
    });

    it("defaults historical previews to a collapsed masked viewport and expands on overlay click", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "collapse_me",
                    renderer: "html",
                    code: "<div>Collapsible</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={2}
                messageId={11}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const expandOverlay = await screen.findByRole("button", { name: "展开预览" });
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Collapsible"));
        expect(host).not.toHaveClass("hidden");
        expect(host).toHaveStyle({
            height: `${PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX}px`,
            minHeight: `${PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX}px`,
            maxHeight: `${PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX}px`,
        });
        expect(host.className).toContain("overflow-hidden");
        expect(host.className).not.toContain("overflow-auto");
        expect(screen.getByText("点击展开预览")).toBeInTheDocument();

        await user.click(expandOverlay);
        await waitFor(() => {
            expect(screen.queryByRole("button", { name: "展开预览" })).not.toBeInTheDocument();
        });
        expect(await screen.findByRole("button", { name: "收起" })).toBeInTheDocument();
        expect(host.style.height).toBe("");
        expect(host.style.minHeight).toBe("");
        expect(host.style.maxHeight).toBe("");
        expect(invoke).not.toHaveBeenCalledWith(
            "submit_preview_code_response",
            expect.anything()
        );
    });

    it("keeps the last message preview expanded by default and supports local hide and show", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);
        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "latest_preview",
                    renderer: "html",
                    code: "<div>Latest Preview</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={6}
                messageId={14}
                isLastMessage
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(host.shadowRoot?.textContent).toContain("Latest Preview")
        );
        expect(host.style.height).toBe("");
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "展开预览" })).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "收起" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "隐藏" }));
        expect(await screen.findByText("预览已隐藏。")).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "显示" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "显示" }));
        expect(screen.queryByText("预览已隐藏。")).not.toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        const restoredHost = await screen.findByTestId("preview-code-host");
        await waitFor(() =>
            expect(restoredHost.shadowRoot?.textContent).toContain("Latest Preview")
        );
    });

    it("keeps hide available while collapsed and can show the preview again", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "collapsed_hide",
                    renderer: "html",
                    code: "<div>Collapsed Hide</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={8}
                messageId={16}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Collapsed Hide"));
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "展开预览" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "隐藏" }));
        expect(await screen.findByText("预览已隐藏。")).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "显示" })).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "显示" }));
        expect(await screen.findByRole("button", { name: "隐藏" })).toBeInTheDocument();
        expect(await screen.findByRole("button", { name: "展开预览" })).toBeInTheDocument();
    });

    it("does not render a manual dismiss action for display-only previews", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "display_only",
                    renderer: "html",
                    code: "<div>Display Only</div>",
                    interaction_mode: "none",
                })}
                conversationId={5}
                messageId={13}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        expect(await screen.findByText("display_only")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "关闭并继续" })).not.toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith(
            "submit_preview_code_response",
            expect.anything()
        );
    });

    it("keeps scripted historical previews static while collapsed and enables interaction after expand", async () => {
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => []);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "script_preview",
                    renderer: "html",
                    code: "<div>Static First</div><script>const marker = document.createElement('div'); marker.textContent = 'Script Active'; marker.setAttribute('data-script-active', 'true'); document.body.appendChild(marker);</script>",
                    interaction_mode: "submit_once",
                })}
                conversationId={7}
                messageId={15}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const host = await screen.findByTestId("preview-code-host");
        await screen.findByText("点击展开预览");
        expect(host.shadowRoot?.textContent).toContain("Static First");
        expect(host.shadowRoot?.querySelector('[data-script-active="true"]')).toBeNull();

        await user.click(screen.getByRole("button", { name: "展开预览" }));

        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Script Active"));
        expect(screen.getByRole("button", { name: "收起" })).toBeInTheDocument();
    });
});
