import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConversationStatsDialog } from "./ConversationStatsDialog";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

describe("ConversationStatsDialog", () => {
    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("shows estimated badge and ACP session usage", async () => {
        mockInvokeHandler("get_conversation_token_stats", () => ({
            total_tokens: 500,
            input_tokens: 180,
            output_tokens: 220,
            thought_tokens: 80,
            cached_read_tokens: 15,
            cached_write_tokens: 5,
            estimated_message_count: 2,
            by_model: [
                {
                    model_id: 0,
                    model_name: "acp",
                    total_tokens: 500,
                    input_tokens: 180,
                    output_tokens: 220,
                    thought_tokens: 80,
                    cached_read_tokens: 15,
                    cached_write_tokens: 5,
                    message_count: 3,
                    avg_ttft_ms: null,
                    avg_tps: null,
                    percentage: 100,
                },
            ],
            message_count: 3,
            system_message_count: 0,
            user_message_count: 1,
            response_message_count: 1,
            reasoning_message_count: 1,
            tool_result_message_count: 0,
            avg_ttft_ms: null,
            avg_tps: null,
            start_time: null,
            finish_time: null,
        }));

        mockInvokeHandler("get_acp_session_state", () => ({
            conversation_id: 42,
            session_id: "session-1",
            title: null,
            updated_at: null,
            load_session_supported: true,
            session_resume_supported: true,
            restored_session_method: "resume",
            prompt_capabilities: { image: true, audio: false, embedded_context: true },
            current_mode_id: null,
            modes: [],
            config_options: [],
            plan: [],
            available_commands: [],
            has_active_prompt: false,
            context_tokens_used: 2048,
            context_window_size: 8192,
            session_cost_amount: 0.1234,
            session_cost_currency: "USD",
        }));

        render(<ConversationStatsDialog conversationId="42" />);

        fireEvent.click(screen.getByRole("button"));

        await waitFor(() => {
            expect(screen.getByText("Token 用量")).toBeInTheDocument();
        });

        expect(screen.getByText("含估算 2 条")).toBeInTheDocument();
        expect(screen.getByText("ACP 会话 Usage")).toBeInTheDocument();
        expect(screen.getByText("2,048")).toBeInTheDocument();
        expect(screen.getByText("8,192")).toBeInTheDocument();
    });
});
