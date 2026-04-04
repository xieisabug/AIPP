import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi, beforeEach } from "vitest";
import { clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import MessageItem from "@/components/MessageItem";
import type { Message, StreamEvent } from "@/data/Conversation";

// --- Mutable display config for per-test overrides ---
const mockDisplayConfig = {
    isUserMessageMarkdownEnabled: true,
    isMergeAssistantMessages: true,
    isShowThinking: true,
    isPreviewCodeShowToolbar: true,
};

vi.mock("@/hooks/useDisplayConfig", () => ({
    useDisplayConfig: () => ({
        config: null,
        isLoading: false,
        error: null,
        ...mockDisplayConfig,
        refreshConfig: vi.fn(),
    }),
}));

vi.mock("@/contexts/AntiLeakageContext", () => ({
    useAntiLeakage: () => ({
        enabled: false,
        isRevealed: true,
    }),
}));

vi.mock("@/components/magicui/shine-border", () => ({
    ShineBorder: () => <div data-testid="shine-border" />,
}));

// Helper to create test messages
function makeMessage(overrides: Partial<Message> = {}): Message {
    return {
        id: 1,
        conversation_id: 1,
        message_type: "response",
        content: "Hello world",
        llm_model_id: null,
        token_count: null,
        input_token_count: null,
        output_token_count: null,
        created_at: "2024-01-01T00:00:00Z",
        finish_time: null,
        attachment_list: [],
        is_context_edge: null,
        generation_group_id: null,
        generation_index: null,
        ...overrides,
    } as Message;
}

describe("Display Config Features", () => {
    beforeEach(() => {
        // Reset to defaults before each test
        mockDisplayConfig.isUserMessageMarkdownEnabled = true;
        mockDisplayConfig.isMergeAssistantMessages = true;
        mockDisplayConfig.isShowThinking = true;
        mockDisplayConfig.isPreviewCodeShowToolbar = true;
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    // ------------------------------------------------------------------
    // Feature 2: Show Thinking toggle
    // ------------------------------------------------------------------
    describe("Show Thinking toggle", () => {
        it("renders full reasoning message when show_thinking is enabled", () => {
            mockDisplayConfig.isShowThinking = true;

            const reasoningMsg = makeMessage({
                id: 10,
                message_type: "reasoning",
                content: "Let me think about this...",
                finish_time: null,
            });

            const { container } = render(
                <MessageItem
                    message={reasoningMsg}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                />
            );

            // Should NOT show the Loader2 spinner badge (animate-spin)
            const spinner = container.querySelector('.animate-spin');
            expect(spinner).not.toBeInTheDocument();
            // Should render reasoning content (the ReasoningMessage component)
            const el = container.querySelector('[data-message-type="reasoning"]');
            expect(el).toBeInTheDocument();
        });

        it("shows thinking badge when reasoning is in progress and show_thinking disabled", () => {
            mockDisplayConfig.isShowThinking = false;

            const reasoningMsg = makeMessage({
                id: 10,
                message_type: "reasoning",
                content: "Let me think...",
                finish_time: null, // still thinking
            });

            render(
                <MessageItem
                    message={reasoningMsg}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                />
            );

            // Should show the loading badge
            expect(screen.getByText("思考中...")).toBeInTheDocument();
        });

        it("hides reasoning entirely when reasoning is complete and show_thinking disabled", () => {
            mockDisplayConfig.isShowThinking = false;

            const completedReasoning = makeMessage({
                id: 10,
                message_type: "reasoning",
                content: "I thought about it.",
                finish_time: new Date("2024-01-01T00:01:00Z"), // completed
            });

            const { container } = render(
                <MessageItem
                    message={completedReasoning}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                />
            );

            // Should not show the loading badge
            expect(screen.queryByText("思考中...")).not.toBeInTheDocument();
            // Should not render the reasoning content at all
            const el = container.querySelector('[data-message-type="reasoning"]');
            expect(el).not.toBeInTheDocument();
        });

        it("hides reasoning via stream is_done when show_thinking disabled", () => {
            mockDisplayConfig.isShowThinking = false;

            const reasoningMsg = makeMessage({
                id: 10,
                message_type: "reasoning",
                content: "Thinking...",
                finish_time: null,
            });

            const { container } = render(
                <MessageItem
                    message={reasoningMsg}
                    streamEvent={{ message_id: 10, message_type: "reasoning", content: "Thinking...", is_done: true } as StreamEvent}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                />
            );

            // Stream is_done means reasoning complete — should render nothing
            expect(screen.queryByText("思考中...")).not.toBeInTheDocument();
            const el = container.querySelector('[data-message-type="reasoning"]');
            expect(el).not.toBeInTheDocument();
        });
    });

    // ------------------------------------------------------------------
    // Feature 1: Merged mode (MessageItem behavior)
    // ------------------------------------------------------------------
    describe("Merged mode (MessageItem)", () => {
        it("renders content without bubble wrapper in merged mode", () => {
            const msg = makeMessage({
                id: 20,
                message_type: "response",
                content: "Merged content test",
            });

            const { container } = render(
                <MessageItem
                    message={msg}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                    mergedMode
                />
            );

            const el = container.querySelector('[data-message-type="response"]');
            expect(el).toBeInTheDocument();

            // In merged mode, there should be no bubble (rounded-2xl border)
            const bubble = container.querySelector('.rounded-2xl.border');
            expect(bubble).not.toBeInTheDocument();
        });

        it("renders bubble wrapper in normal (non-merged) mode", () => {
            const msg = makeMessage({
                id: 21,
                message_type: "response",
                content: "Normal content test",
            });

            const { container } = render(
                <MessageItem
                    message={msg}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                    mergedMode={false}
                />
            );

            // In normal mode, should have the bubble wrapper
            const bubble = container.querySelector('.rounded-2xl');
            expect(bubble).toBeInTheDocument();
        });

        it("still renders bubble for user messages even in merged mode", () => {
            const msg = makeMessage({
                id: 22,
                message_type: "user",
                content: "User says hi",
            });

            const { container } = render(
                <MessageItem
                    message={msg}
                    conversationId={1}
                    mcpToolCallStates={new Map()}
                    mergedMode
                />
            );

            // User messages should always have the bubble wrapper
            const bubble = container.querySelector('.rounded-2xl');
            expect(bubble).toBeInTheDocument();
        });
    });
});
