import { render, screen } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import VirtuosoMessageList, { getVirtuosoRowMinHeight } from "./VirtuosoMessageList";
import {
    CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR,
} from "./layoutConstants";
import type { Message } from "@/data/Conversation";

vi.mock("react-virtuoso", () => ({
    Virtuoso: ({ components, context }: any) => (
        <div data-testid="virtuoso">
            {components.Footer ? <components.Footer context={context} /> : null}
        </div>
    ),
}));

vi.mock("@/hooks/useDisplayConfig", () => ({
    useDisplayConfig: () => ({
        isMergeAssistantMessages: true,
    }),
}));

vi.mock("@/hooks/useFeishuDebugResend", () => ({
    useFeishuDebugResend: () => ({
        pendingMessageId: null,
        resendMessageToFeishuDebug: vi.fn(),
    }),
}));

vi.mock("../MessageItem", () => ({
    default: ({ message }: { message: Message }) => (
        <div data-testid={`message-${message.id}`}>{message.content}</div>
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

class ResizeObserverMock {
    observe = vi.fn();
    disconnect = vi.fn();
}

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

function makeProps(messages: Message[]) {
    return {
        allDisplayMessages: messages,
        streamingMessages: new Map<number, never>(),
        shiningMessageIds: new Set<number>(),
        shiningMcpCallId: null,
        reasoningExpandStates: new Map<number, boolean>(),
        mcpToolCallStates: new Map<number, never>(),
        generationGroups: new Map<string, never>(),
        selectedVersions: new Map<string, number>(),
        getGenerationGroupControl: () => null,
        handleGenerationVersionChange: () => undefined,
        onCodeRun: () => undefined,
        onMessageRegenerate: () => undefined,
        onMessageEdit: () => undefined,
        onMessageFork: () => undefined,
        onToggleReasoningExpand: () => undefined,
        scrollContainerRef: {
            current: document.createElement("div"),
        } as React.RefObject<HTMLDivElement | null>,
        pendingScrollMessageId: null,
        clearPendingScrollMessageId: vi.fn(),
        setShiningMessageIds: vi.fn(),
        smartScroll: vi.fn(),
    };
}

describe("VirtuosoMessageList row height reservation", () => {
    beforeEach(() => {
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
    });

    afterEach(() => {
        vi.unstubAllGlobals();
    });

    it("keeps the first history row's estimated min height", () => {
        expect(
            getVirtuosoRowMinHeight(0, { estimatedHeight: 240 }),
        ).toBe(240);
    });

    it("does not force estimated min height on later history rows", () => {
        expect(
            getVirtuosoRowMinHeight(1, { estimatedHeight: 240 }),
        ).toBeUndefined();
    });

    it("does not override the footer viewport height after history items exist", () => {
        render(
            <VirtuosoMessageList
                {...makeProps([
                    makeMessage({
                        id: 1,
                        message_type: "user",
                        content: "user-1",
                    }),
                    makeMessage({
                        id: 2,
                        message_type: "response",
                        content: "response-1",
                    }),
                    makeMessage({
                        id: 3,
                        message_type: "user",
                        content: "user-2",
                    }),
                ])}
            />,
        );

        const lastReplyContainer = screen
            .getByTestId("message-3")
            .closest("[data-aipp-slot='chat-last-reply-container']") as HTMLElement;
        const footer = lastReplyContainer.parentElement as HTMLElement;

        expect(footer.style.getPropertyValue(CHAT_SCROLL_VIEWPORT_HEIGHT_CSS_VAR)).toBe("");
    });
});
