import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MessageTokenTooltip } from "./MessageTokenTooltip";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

describe("MessageTokenTooltip", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("loads and shows reported usage with cache fields when opened", async () => {
        mockInvokeHandler("get_message_token_stats", () => ({
            message_id: 7,
            total_tokens: 120,
            input_tokens: 40,
            output_tokens: 50,
            thought_tokens: 20,
            cached_read_tokens: 8,
            cached_write_tokens: 2,
            usage_source: "reported",
            model_name: "acp",
            ttft_ms: 320,
            tps: 11.2,
            start_time: "2026-05-06T01:00:00.000Z",
            finish_time: "2026-05-06T01:00:10.000Z",
        }));

        render(<MessageTokenTooltip messageId={7} messageType="response" />);

        fireEvent.click(screen.getByRole("button"));

        await waitFor(() => {
            expect(screen.getByText("消息 Token")).toBeInTheDocument();
        });

        expect(screen.getByText("精确")).toBeInTheDocument();
        expect(screen.getByText("120")).toBeInTheDocument();
        expect(screen.getByText("20")).toBeInTheDocument();
        expect(screen.getByText("8")).toBeInTheDocument();
        expect(screen.getByText("2")).toBeInTheDocument();
    });

    it("shows estimated badge for estimated usage", async () => {
        mockInvokeHandler("get_message_token_stats", () => ({
            message_id: 9,
            total_tokens: 88,
            input_tokens: 30,
            output_tokens: 40,
            thought_tokens: 18,
            cached_read_tokens: 0,
            cached_write_tokens: 0,
            usage_source: "estimated",
            model_name: "acp",
            ttft_ms: null,
            tps: null,
            start_time: null,
            finish_time: null,
        }));

        render(<MessageTokenTooltip messageId={9} messageType="response" />);

        fireEvent.click(screen.getByRole("button"));

        await waitFor(() => {
            expect(screen.getByText("估算")).toBeInTheDocument();
        });
    });
});
