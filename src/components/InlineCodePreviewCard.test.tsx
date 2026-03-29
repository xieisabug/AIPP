import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import InlineCodePreviewCard from "@/components/InlineCodePreviewCard";

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
        expect(host.shadowRoot?.querySelector("style")?.textContent).toContain(":host");
        expect(host.querySelector("style")).toBeNull();
    });

    it("toggles collapse locally without dismissing the preview request", async () => {
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
        const toggleButton = await screen.findByRole("button", { name: "收起" });
        const host = await screen.findByTestId("preview-code-host");
        await waitFor(() => expect(host.shadowRoot?.textContent).toContain("Collapsible"));
        expect(host).not.toHaveClass("hidden");

        await user.click(toggleButton);
        expect(await screen.findByRole("button", { name: "展开" })).toBeInTheDocument();
        expect(await screen.findByText("预览已收起。")).toBeInTheDocument();
        expect(host).toHaveClass("hidden");
        expect(invoke).not.toHaveBeenCalledWith(
            "submit_preview_code_response",
            expect.anything()
        );

        await user.click(screen.getByRole("button", { name: "展开" }));
        expect(await screen.findByRole("button", { name: "收起" })).toBeInTheDocument();
        expect(screen.queryByText("预览已收起。")).not.toBeInTheDocument();
        expect(host).not.toHaveClass("hidden");
    });
});

