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

    it("submits a dismissed result when close is clicked after request recovery", async () => {
        const requestId = "preview-req-1";
        mockInvokeHandler("list_preview_code_requests_for_conversation", () => [
            {
                request_id: requestId,
                conversation_id: 2,
                title: "close_me",
                renderer: "html",
                code: "<div>Closable</div>",
                interactionMode: "submit_once",
                loadingMessages: [],
            },
        ]);
        mockInvokeHandler("submit_preview_code_response", () => true);

        render(
            <InlineCodePreviewCard
                parameters={JSON.stringify({
                    title: "close_me",
                    renderer: "html",
                    code: "<div>Closable</div>",
                    interaction_mode: "submit_once",
                })}
                conversationId={2}
                messageId={11}
                mcpToolCallStates={new Map()}
                isStreaming={false}
            />
        );

        const user = userEvent.setup();
        const closeButton = await screen.findByRole("button", { name: "关闭" });
        await waitFor(() => expect(closeButton).toBeEnabled());
        await user.click(closeButton);

        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("submit_preview_code_response", {
                requestId,
                request_id: requestId,
                payload: null,
                dismissed: true,
            })
        );
        expect(await screen.findByText("该内嵌 UI 已关闭。")).toBeInTheDocument();
    });
});

