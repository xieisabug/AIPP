import React from "react";
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { AssistantListItem } from "@/data/Assistant";
import type { Message, StreamEvent } from "@/data/Conversation";
import type { InlineInteractionItem } from "../ConversationUI";
import ConversationContent from "./ConversationContent";

vi.mock("./MessageList", () => ({
    default: () => <div data-testid="legacy-message-list" />,
}));

vi.mock("./VirtualizedMessageList", () => ({
    default: () => <div data-testid="legacy-virtualized-list" />,
}));

vi.mock("./VirtuosoMessageList", () => ({
    default: () => <div data-testid="virtuoso-message-list" />,
}));

vi.mock("../NewChatComponent", () => ({
    default: () => <div data-testid="new-chat-component" />,
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

function makeProps(
    overrides: Partial<React.ComponentProps<typeof ConversationContent>> = {},
) {
    const commonMessages = [makeMessage({ id: 1, message_type: "user" })];
    const streamingMessages = new Map<number, StreamEvent>();
    const shiningMessageIds = new Set<number>();
    const reasoningExpandStates = new Map<number, boolean>();
    const mcpToolCallStates = new Map<number, unknown>();
    const generationGroups = new Map<string, unknown>();
    const selectedVersions = new Map<string, number>();
    const inlineInteractionItems: InlineInteractionItem[] = [];
    const assistants: AssistantListItem[] = [];

    return {
        conversationId: "1",
        allDisplayMessages: commonMessages,
        streamingMessages,
        shiningMessageIds,
        shiningMcpCallId: null,
        reasoningExpandStates,
        mcpToolCallStates,
        generationGroups,
        selectedVersions,
        getGenerationGroupControl: () => null,
        handleGenerationVersionChange: () => undefined,
        onCodeRun: () => undefined,
        onMessageRegenerate: () => undefined,
        onMessageEdit: () => undefined,
        onMessageFork: () => undefined,
        onQueuedMessagePromote: () => undefined,
        onToggleReasoningExpand: () => undefined,
        inlineInteractionItems,
        allowFeishuDebugResend: false,
        virtualizeMessages: true,
        scrollContainerRef: {
            current: document.createElement("div"),
        } as React.RefObject<HTMLDivElement | null>,
        pendingScrollMessageId: null,
        clearPendingScrollMessageId: vi.fn(),
        setShiningMessageIds: vi.fn(),
        onScrollStateChange: vi.fn(),
        smartScroll: vi.fn(),
        selectedText: "",
        selectedAssistant: -1,
        assistants,
        setSelectedAssistant: vi.fn(),
        ...overrides,
    };
}

describe("ConversationContent virtualization engine", () => {
    it("keeps the legacy virtualized list as the default engine", () => {
        render(<ConversationContent {...makeProps()} />);

        expect(screen.getByTestId("legacy-virtualized-list")).toBeInTheDocument();
        expect(screen.queryByTestId("virtuoso-message-list")).not.toBeInTheDocument();
    });

    it("switches to the Virtuoso core list when requested", () => {
        render(
            <ConversationContent
                {...makeProps({ virtualizedListEngine: "virtuoso" })}
            />,
        );

        expect(screen.getByTestId("virtuoso-message-list")).toBeInTheDocument();
        expect(screen.queryByTestId("legacy-virtualized-list")).not.toBeInTheDocument();
    });
});
