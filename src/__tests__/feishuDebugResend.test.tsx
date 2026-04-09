import { fireEvent, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useMessageListElements } from "@/components/conversation/useMessageListElements";
import type { Message } from "@/data/Conversation";

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
    ShineBorder: () => null,
}));

function makeMessage(overrides: Partial<Message> = {}): Message {
    return {
        id: 1,
        conversation_id: 1,
        message_type: "response",
        content: "Hello world",
        llm_model_id: null,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        created_at: "2024-01-01T00:00:00Z",
        finish_time: "2024-01-01T00:00:01Z",
        attachment_list: [],
        is_context_edge: null,
        generation_group_id: null,
        generation_index: null,
        ...overrides,
    } as Message;
}

function UseMessageListElementsHarness({ messages, allowFeishuDebugResend = false }: {
    messages: Message[];
    allowFeishuDebugResend?: boolean;
}) {
    const { messageElements } = useMessageListElements({
        allDisplayMessages: messages,
        streamingMessages: new Map(),
        shiningMessageIds: new Set(),
        shiningMcpCallId: null,
        reasoningExpandStates: new Map(),
        mcpToolCallStates: new Map(),
        generationGroups: new Map(),
        selectedVersions: new Map(),
        getGenerationGroupControl: () => null,
        handleGenerationVersionChange: () => undefined,
        onCodeRun: () => undefined,
        onMessageRegenerate: () => undefined,
        onMessageEdit: () => undefined,
        onMessageFork: () => undefined,
        onToggleReasoningExpand: () => undefined,
        allowFeishuDebugResend,
    });

    return (
        <>
            {messageElements.map(({ messageId, messageElement }) => (
                <div key={messageId}>{messageElement}</div>
            ))}
        </>
    );
}

describe("Feishu debug resend", () => {
    beforeEach(() => {
        mockDisplayConfig.isMergeAssistantMessages = true;
    });

    afterEach(() => {
        clearAllMockHandlers();
        vi.clearAllMocks();
    });

    it("uses the latest tool_result as resend target in merged assistant bubbles", async () => {
        const invokeSpy = vi.fn((args?: Record<string, unknown>) => {
            expect(args).toMatchObject({
                messageId: 11,
                message_id: 11,
            });

            return {
                external_message_id: "om_xxx",
                payload_type: "interactive",
                part_count: 1,
                interactive_part_count: 1,
                text_part_count: 0,
                delivery_mode: "reply",
                rendered_text: "converted preview content",
            };
        });
        mockInvokeHandler("debug_resend_message_to_feishu", invokeSpy);

        const responseMessage = makeMessage({
            id: 10,
            message_type: "response",
            content: "修改完成。<!-- MCP_TOOL_CALL:{\"tool_name\":\"preview_file\"} -->",
        });
        const toolResultMessage = makeMessage({
            id: 11,
            message_type: "tool_result",
            content: "Tool execution completed:\n\nTool Call ID: call_1\nTool: preview_file",
        });

        const { container } = render(
            <UseMessageListElementsHarness
                messages={[responseMessage, toolResultMessage]}
                allowFeishuDebugResend
            />
        );

        const resendButton = container.querySelector('[data-aipp-slot="message-toolbar-resend-feishu"]');
        expect(resendButton).toBeInTheDocument();

        fireEvent.click(resendButton as HTMLElement);

        await waitFor(() => {
            expect(invokeSpy).toHaveBeenCalledTimes(1);
        });
    });
});
