import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useMessageListElements } from "./useMessageListElements";
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

vi.mock("@/hooks/useFeishuDebugResend", () => ({
    useFeishuDebugResend: () => ({
        pendingMessageId: null,
        resendMessageToFeishuDebug: vi.fn(),
    }),
}));

vi.mock("../MessageItem", () => ({
    default: ({
        message,
        isLastMessage = false,
        mergedMode = false,
    }: {
        message: Message;
        isLastMessage?: boolean;
        mergedMode?: boolean;
    }) => (
        <div
            data-testid={`message-${message.id}`}
            data-last-message={isLastMessage ? "true" : "false"}
            data-merged-mode={mergedMode ? "true" : "false"}
        >
            {message.content}
        </div>
    ),
}));

vi.mock("../VersionPagination", () => ({
    default: () => null,
}));

vi.mock("../magicui/shine-border", () => ({
    ShineBorder: () => null,
}));

vi.mock("../message-item/MessageActionButtons", () => ({
    default: () => null,
}));

function makeMessage(overrides: Partial<Message>): Message {
    return {
        id: 1,
        conversation_id: 1,
        message_type: "response",
        content: "message",
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

function Harness({ messages }: { messages: Message[] }) {
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
    });

    return (
        <>
            {messageElements.map(({ messageId, messageElement }) => (
                <div key={messageId}>{messageElement}</div>
            ))}
        </>
    );
}

describe("useMessageListElements merged assistant preview state", () => {
    afterEach(() => {
        vi.clearAllMocks();
    });

    it("treats every message in the trailing merged assistant group as the last message", () => {
        render(
            <Harness
                messages={[
                    makeMessage({ id: 1, message_type: "user", content: "user" }),
                    makeMessage({ id: 2, message_type: "response", content: "response" }),
                    makeMessage({ id: 3, message_type: "tool_result", content: "tool_result" }),
                ]}
            />
        );

        expect(screen.getByTestId("message-1")).toHaveAttribute("data-last-message", "false");
        expect(screen.getByTestId("message-2")).toHaveAttribute("data-last-message", "true");
        expect(screen.getByTestId("message-2")).toHaveAttribute("data-merged-mode", "true");
        expect(screen.getByTestId("message-3")).toHaveAttribute("data-last-message", "true");
        expect(screen.getByTestId("message-3")).toHaveAttribute("data-merged-mode", "true");
    });

    it("stops treating a merged assistant group as last after a following user message arrives", () => {
        render(
            <Harness
                messages={[
                    makeMessage({ id: 1, message_type: "user", content: "user-1" }),
                    makeMessage({ id: 2, message_type: "response", content: "response" }),
                    makeMessage({ id: 3, message_type: "tool_result", content: "tool_result" }),
                    makeMessage({ id: 4, message_type: "user", content: "user-2" }),
                ]}
            />
        );

        expect(screen.getByTestId("message-2")).toHaveAttribute("data-last-message", "false");
        expect(screen.getByTestId("message-3")).toHaveAttribute("data-last-message", "false");
        expect(screen.getByTestId("message-4")).toHaveAttribute("data-last-message", "true");
    });
});
