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

function makeProps(messages: Message[], forceVirtualized = false) {
    return {
        conversationId: "1",
        allDisplayMessages: forceVirtualized
            ? [...Array.from({ length: 81 }, (_, i) => makeMessage({ id: 10000 + i, message_type: "user" })), ...messages]
            : messages,
        streamingMessages: new Map<number, any>(),
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

    it("should keep bounded history mounted when messages are rich but few", () => {
        render(<VirtuosoMessageList {...makeProps([
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2 }),
            makeMessage({ id: 3, message_type: "user" }),
            makeMessage({ id: 4 }),
        ])} />);
        expect(screen.queryByTestId("virtuoso")).not.toBeInTheDocument();
        for (const id of [1, 2, 3, 4]) expect(screen.getByTestId(`message-${id}`)).toBeInTheDocument();
    });

    it("should keep virtualization when a few messages exceed the text budget", () => {
        render(<VirtuosoMessageList {...makeProps([
            makeMessage({ id: 1, message_type: "user", content: "x".repeat(500001) }),
            makeMessage({ id: 2 }),
            makeMessage({ id: 3, message_type: "user" }),
        ])} />);
        expect(screen.getByTestId("virtuoso")).toBeInTheDocument();
    });

    it("should keep retained history visible when the scroll parent is unavailable", () => {
        render(<VirtuosoMessageList {...makeProps([
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2 }),
            makeMessage({ id: 3, message_type: "user" }),
            makeMessage({ id: 4 }),
        ])} scrollContainerRef={{ current: null }} />);
        expect(screen.queryByTestId("virtuoso")).not.toBeInTheDocument();
        expect(screen.getByTestId("message-1")).toBeVisible();
        expect(screen.getByTestId("message-4")).toBeVisible();
        expect(document.querySelector("[data-aipp-initial-bottom-positioning='true']")).toBeNull();
    });

    it("should navigate to mounted history when a message is selected", async () => {
        const props = makeProps([
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2 }),
            makeMessage({ id: 3, message_type: "user" }),
            makeMessage({ id: 4 }),
        ]);
        const scrollIntoView = vi.fn();
        const target = document.createElement("div");
        target.dataset.messageId = "1";
        target.scrollIntoView = scrollIntoView;
        props.scrollContainerRef.current!.append(target);

        render(<VirtuosoMessageList {...props} pendingScrollMessageId={1} />);

        expect(screen.queryByTestId("virtuoso")).not.toBeInTheDocument();
        expect(scrollIntoView).toHaveBeenCalledWith({ block: "center", behavior: "smooth" });
        await waitFor(() => expect(props.clearPendingScrollMessageId).toHaveBeenCalledWith(null));
        expect(props.setShiningMessageIds).toHaveBeenCalled();
        expect(document.querySelector("[data-aipp-initial-bottom-positioning='true']")).toBeNull();
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
                ], true)}
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
                ], true)}
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
                ], true)}
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
    it("keeps sticking to bottom after pin effect restarts and content grows late", async () => {
        let scrollHeight = 1000;
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            get: () => scrollHeight,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });

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
                {...makeProps(messages, true)}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                onScrollStateChange={vi.fn()}
            />,
        );

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });

        // 模拟父组件重渲染导致 onScrollStateChange identity 变化 → effect cleanup 停掉 follow
        rerender(
            <VirtuosoMessageList
                {...makeProps(messages, true)}
                scrollContainerRef={{
                    current: scrollContainer,
                }}
                onScrollStateChange={vi.fn()}
            />,
        );

        // 晚到的布局增高（代码高亮 / 图片 / Virtuoso 估高校正）
        scrollHeight = 2000;
        scrollContainer.scrollTop = 600;
        scrollContainer.dispatchEvent(new Event("scroll"));

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(1600);
        });
    });


    it("keeps sticking to bottom while streaming content grows", async () => {
        let scrollHeight = 1000;
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            get: () => scrollHeight,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });

        const streamEvent = {
            message_id: 99,
            conversation_id: 1,
            content: "stream-start",
            is_done: false,
            reasoning_content: null,
        } as any;

        const baseMessages = [
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
                content: "ask-long",
            }),
        ];

        const props = makeProps(baseMessages, true);
        props.streamingMessages = new Map([[99, streamEvent]]);

        const { rerender } = render(
            <VirtuosoMessageList
                {...props}
                scrollContainerRef={{ current: scrollContainer }}
                onScrollStateChange={vi.fn()}
            />,
        );

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });

        // 流式 token 导致内容持续增高（无用户上滑）
        scrollHeight = 2500;
        scrollContainer.scrollTop = 600;
        scrollContainer.dispatchEvent(new Event("scroll"));

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(2100);
        });

        // 流式结束后不再强制跟（清理 follow）；这里只验证流式期间贴底
        const doneProps = makeProps(baseMessages, true);
        doneProps.streamingMessages = new Map();
        rerender(
            <VirtuosoMessageList
                {...doneProps}
                scrollContainerRef={{ current: scrollContainer }}
                onScrollStateChange={vi.fn()}
            />,
        );
    });

    it("pauses stream follow when user wheels up, resumes after return to bottom", async () => {
        let scrollHeight = 1000;
        const scrollContainer = document.createElement("div");
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            get: () => scrollHeight,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });

        const streamEvent = {
            message_id: 99,
            conversation_id: 1,
            content: "stream-start",
            is_done: false,
            reasoning_content: null,
        } as any;

        const baseMessages = [
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
                content: "ask-long",
            }),
        ];

        const props = makeProps(baseMessages, true);
        props.streamingMessages = new Map([[99, streamEvent]]);

        render(
            <VirtuosoMessageList
                {...props}
                scrollContainerRef={{ current: scrollContainer }}
                onScrollStateChange={vi.fn()}
            />,
        );

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });

        // 等初始 pin 结束，避免 pin 的 scroll 监听把用户上滑抢回底部
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

        // 用户上滑：follow 应暂停
        scrollContainer.dispatchEvent(new WheelEvent("wheel", { deltaY: -100 }));
        scrollContainer.scrollTop = 200;
        scrollContainer.dispatchEvent(new Event("scroll"));

        scrollHeight = 2200;
        // 触发 height poll / 程序 scroll 路径
        await new Promise((r) => setTimeout(r, 150));
        expect(scrollContainer.scrollTop).toBe(200);

        // 滚回底部 → 恢复 follow
        scrollContainer.scrollTop = 2200 - 400;
        scrollContainer.dispatchEvent(new Event("scroll"));

        scrollHeight = 3000;
        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(2600);
        }, { timeout: 2000 });
    });

});
