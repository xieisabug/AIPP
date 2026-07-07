import { render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import VirtuosoMessageList, { getVirtuosoRowMinHeight } from "./VirtuosoMessageList";
import type { Message } from "@/data/Conversation";

const virtuosoMockState = vi.hoisted(() => ({
    lastProps: null as any,
}));

vi.mock("react-virtuoso", () => ({
    Virtuoso: (props: any) => {
        virtuosoMockState.lastProps = props;
        const { components, context } = props;
        return (
            <div data-testid="virtuoso">
                {props.data.length > 0 && components.Footer
                    ? <components.Footer context={context} />
                    : null}
            </div>
        );
    },
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
        conversationId: "1",
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
        virtuosoMockState.lastProps = null;
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

    it("starts new conversation renders at the bottom without waiting for outer smartScroll", async () => {
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            value: 1000,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });
        const smartScroll = vi.fn();
        const onScrollStateChange = vi.fn();

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
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                smartScroll={smartScroll}
                onScrollStateChange={onScrollStateChange}
            />,
        );

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });
        expect(virtuosoMockState.lastProps.initialTopMostItemIndex).toMatchObject({
            index: "LAST",
            align: "end",
        });
        expect(virtuosoMockState.lastProps.initialTopMostItemIndex.offset).toBeLessThan(0);
        expect(smartScroll).not.toHaveBeenCalled();
        expect(onScrollStateChange).toHaveBeenCalledWith(scrollContainer);
    });

    it("does not pin to bottom after a pending message scroll is cleared", () => {
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            value: 1000,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });
        scrollContainer.scrollTop = 125;
        const messages = [
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
        ];

        const { rerender } = render(
            <VirtuosoMessageList
                {...makeProps(messages)}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                pendingScrollMessageId={1}
            />,
        );

        rerender(
            <VirtuosoMessageList
                {...makeProps(messages)}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                pendingScrollMessageId={null}
            />,
        );

        expect(scrollContainer.scrollTop).toBe(125);
    });

    it("pins to bottom after a new user message is appended", async () => {
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            value: 1000,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });
        const smartScroll = vi.fn();
        const onScrollStateChange = vi.fn();
        const initialMessages = [
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
        ];

        const { rerender } = render(
            <VirtuosoMessageList
                {...makeProps(initialMessages)}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                smartScroll={smartScroll}
                onScrollStateChange={onScrollStateChange}
            />,
        );

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });

        scrollContainer.scrollTop = 125;

        rerender(
            <VirtuosoMessageList
                {...makeProps([
                    ...initialMessages,
                    makeMessage({
                        id: 4,
                        message_type: "response",
                        content: "response-2",
                    }),
                    makeMessage({
                        id: 5,
                        message_type: "user",
                        content: "user-3",
                    }),
                ])}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                smartScroll={smartScroll}
                onScrollStateChange={onScrollStateChange}
            />,
        );

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });
        expect(smartScroll).not.toHaveBeenCalled();
    });

    it("includes live footer estimate in initial bottom alignment", () => {
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            value: 1000,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });

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
                    makeMessage({
                        id: 4,
                        message_type: "response",
                        content: "response-2",
                    }),
                ])}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
            />,
        );

        expect(virtuosoMockState.lastProps.initialTopMostItemIndex).toMatchObject({
            index: "LAST",
            align: "end",
        });
        expect(virtuosoMockState.lastProps.initialTopMostItemIndex.offset).toBeLessThan(0);
    });

    it("renders the latest Butler user message and error without hiding the live footer", () => {
        const { unmount } = render(
            <VirtuosoMessageList
                {...makeProps([
                    makeMessage({
                        id: 9985,
                        message_type: "user",
                        content: "你好",
                    }),
                    makeMessage({
                        id: 9986,
                        message_type: "error",
                        content: "No available providers",
                    }),
                ])}
            />,
        );

        expect(screen.getByTestId("message-9985")).toBeInTheDocument();
        expect(screen.getByTestId("message-9986")).toBeInTheDocument();
        expect(screen.queryByTestId("virtuoso")).not.toBeInTheDocument();
        expect(
            document.querySelector("[data-aipp-initial-bottom-positioning='true']"),
        ).toBeNull();
        unmount();
    });

    it("still renders messages when scrollContainerRef.current is null", () => {
        render(
            <VirtuosoMessageList
                {...makeProps([
                    makeMessage({
                        id: 42,
                        message_type: "user",
                        content: "still-visible",
                    }),
                    makeMessage({
                        id: 43,
                        message_type: "error",
                        content: "provider-error",
                    }),
                ])}
                scrollContainerRef={{ current: null }}
            />,
        );

        // 旧逻辑会卡在 minHeight:1 占位，气泡全无；现在即使 ref.current=null 也要渲染
        expect(screen.getByTestId("message-42")).toBeInTheDocument();
        expect(screen.getByTestId("message-43")).toBeInTheDocument();
        expect(screen.queryByTestId("virtuoso")).not.toBeInTheDocument();
    });

    it("unhides virtuoso history path after initial bottom pin settles", async () => {
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            value: 1000,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });

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
                scrollContainerRef={{
                    current: scrollContainer,
                }}
            />,
        );

        // history 路径允许短暂 hide，但 pin/failsafe 完成后必须露出
        await waitFor(
            () => {
                expect(
                    document.querySelector(
                        "[data-aipp-initial-bottom-positioning='true']",
                    ),
                ).toBeNull();
            },
            { timeout: 2000 },
        );
        expect(screen.getByTestId("virtuoso")).toBeVisible();
    });
});
