import { act, render, screen, waitFor } from "@testing-library/react";
import React from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import VirtuosoMessageList, { getVirtuosoRowMinHeight } from "./VirtuosoMessageList";
import * as sendSlide from "./sendScrollSequence";
import * as sentBubble from "./sentBubbleAnimation";
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
        <div data-message-item data-message-id={message.id} data-testid={`message-${message.id}`}><div>{message.content}</div></div>
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
        historyLoaded: true,
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
        expect(document.querySelector("[style*='content-visibility']")).toBeNull();
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
        document.body.appendChild(scrollContainer);
        const query = scrollContainer.querySelector.bind(scrollContainer);
        vi.spyOn(scrollContainer, "querySelector").mockImplementation((selector: string) => {
            const row = query(selector);
            if (row instanceof HTMLElement) row.getBoundingClientRect = () => ({
                top: 608 - scrollContainer.scrollTop, bottom: 648 - scrollContainer.scrollTop,
                left: 0, right: 400, width: 400, height: 40, x: 0, y: 0, toJSON() {},
            });
            return row;
        });
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
            { container: scrollContainer },
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

    it("keeps the send pin alive when an assistant reply is appended right after sending", async () => {
        const scrollContainer = document.createElement("div");
        document.body.appendChild(scrollContainer);
        const query = scrollContainer.querySelector.bind(scrollContainer);
        vi.spyOn(scrollContainer, "querySelector").mockImplementation((selector: string) => {
            const row = query(selector);
            if (row instanceof HTMLElement) row.getBoundingClientRect = () => ({
                top: 608 - scrollContainer.scrollTop, bottom: 648 - scrollContainer.scrollTop,
                left: 0, right: 400, width: 400, height: 40, x: 0, y: 0, toJSON() {},
            });
            return row;
        });
        Object.defineProperty(scrollContainer, "scrollHeight", {
            configurable: true,
            value: 1000,
        });
        Object.defineProperty(scrollContainer, "clientHeight", {
            configurable: true,
            value: 400,
        });
        const baseMessages = [
            makeMessage({ id: 1, message_type: "user", content: "user-1" }),
            makeMessage({ id: 2, message_type: "response", content: "response-1" }),
        ];
        const renderWith = (messages: Message[]) => (
            <VirtuosoMessageList
                {...makeProps(messages)}
                scrollContainerRef={{ current: scrollContainer }}
            />
        );

        const { rerender } = render(renderWith(baseMessages), { container: scrollContainer });
        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });

        scrollContainer.scrollTop = 100;
        const withUser = [
            ...baseMessages,
            makeMessage({ id: 3, message_type: "user", content: "user-2" }),
        ];
        rerender(renderWith(withUser));

        // Bubble entry preserves the native scroll position before movement begins.
        expect(scrollContainer.scrollTop).toBe(100);

        // 后端紧接着追加 assistant 占位：tail 不再是 user，effect 重跑
        rerender(renderWith([
            ...withUser,
            makeMessage({ id: 4, message_type: "response", content: "" }),
        ]));

        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(600);
        });
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

    it("should pass per-row heightEstimates without defaultItemHeight when history is virtualized", () => {
        const scrollContainer = document.createElement("div");

        render(
            <VirtuosoMessageList
                {...makeProps([
                    makeMessage({ id: 1, message_type: "user", content: "user-1" }),
                    makeMessage({
                        id: 2,
                        message_type: "response",
                        content: "response line\n\n".repeat(30),
                    }),
                    makeMessage({ id: 3, message_type: "user", content: "user-2" }),
                    makeMessage({ id: 4, message_type: "response", content: "response-2" }),
                ], true)}
                scrollContainerRef={{ current: scrollContainer }}
            />,
        );

        const props = virtuosoMockState.lastProps;
        // defaultItemHeight 会先填充 sizeTree，导致 Virtuoso 丢弃逐行 heightEstimates。
        expect(props.defaultItemHeight).toBeUndefined();
        expect(props.heightEstimates).toHaveLength(props.data.length);
        expect(new Set(props.heightEstimates).size).toBeGreaterThan(1);
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

        // 滚回底部（wheel-down force resume）→ 恢复 follow
        scrollContainer.scrollTop = 2200 - 400;
        scrollContainer.dispatchEvent(new WheelEvent("wheel", { deltaY: 120 }));

        scrollHeight = 3000;
        await waitFor(() => {
            expect(scrollContainer.scrollTop).toBe(2600);
        }, { timeout: 2000 });
    });

    it("does not pull the scrollbar back to the bottom after the conversation opens", async () => {
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
                scrollContainerRef={{ current: scrollContainer }}
                onScrollStateChange={vi.fn()}
            />,
        );

        await waitFor(() => {
            expect(
                document.querySelector("[data-aipp-initial-bottom-positioning='true']"),
            ).toBeNull();
        }, { timeout: 2000 });

        // 高度没变，从底部向上拖一段，仍落在宽松恢复阈值内
        scrollContainer.scrollTop = 480;
        scrollContainer.dispatchEvent(new Event("scroll"));
        scrollContainer.scrollTop = 220;
        scrollContainer.dispatchEvent(new Event("scroll"));

        await new Promise((resolve) => setTimeout(resolve, 150));
        expect(scrollContainer.scrollTop).toBe(220);

        rerender(
            <VirtuosoMessageList
                {...makeProps(messages, true)}
                scrollContainerRef={{ current: scrollContainer }}
                onScrollStateChange={vi.fn()}
            />,
        );

        scrollHeight = 1800;
        scrollContainer.dispatchEvent(new Event("scroll"));
        await new Promise((resolve) => setTimeout(resolve, 150));
        expect(scrollContainer.scrollTop).toBe(220);
    });

});


describe("VirtuosoMessageList send ownership", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        vi.stubGlobal("ResizeObserver", ResizeObserverMock);
        vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
            const container = this.closest("[data-sequence-test]") as HTMLElement | null;
            const top = container && this.hasAttribute("data-message-item")
                ? container.scrollHeight - container.clientHeight + 8 - container.scrollTop : 0;
            return { top, bottom: top + 40, left: 0, right: 400, width: 400, height: 40, x: 0, y: top, toJSON() {} };
        });
    });
    afterEach(() => {
        vi.useRealTimers();
        vi.restoreAllMocks();
        vi.unstubAllGlobals();
    });

    function setup(initial: Message[] = [], historyLoaded = true) {
        const container = document.createElement("div");
        container.dataset.sequenceTest = "true";
        document.body.appendChild(container);
        let height = 400;
        Object.defineProperty(container, "scrollHeight", { get: () => height });
        Object.defineProperty(container, "clientHeight", { value: 400 });
        const ref = { current: container };
        const props = { ...makeProps(initial), historyLoaded };
        const view = render(<VirtuosoMessageList {...props} scrollContainerRef={ref} />, { container });
        return {
            container,
            setHeight: (value: number) => { height = value; },
            update: (messages: Message[], extra = {}) => view.rerender(
                <VirtuosoMessageList {...props} allDisplayMessages={messages} scrollContainerRef={ref} {...extra} />,
            ),
            unmount: view.unmount,
        };
    }

    it("should preserve reading position when sync deletion reveals a previously rendered user turn", () => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const slide = vi.spyOn(sendSlide, "startSendScrollSequence");
        const history = [
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2, message_type: "response" }),
            makeMessage({ id: 3, message_type: "user" }),
            makeMessage({ id: 4, message_type: "response" }),
        ];
        const fixture = setup(history);
        fixture.setHeight(2000);
        act(() => { vi.advanceTimersByTime(500); });
        fixture.container.scrollTop = 240;
        fixture.setHeight(1400);
        fixture.update(history.slice(0, 2));
        // A sync refresh restores the old reading position after the child commit.
        fixture.container.scrollTop = 240;
        act(() => { vi.advanceTimersByTime(1000); });
        expect(bubble).not.toHaveBeenCalled();
        expect(slide).not.toHaveBeenCalled();
        expect(fixture.container.scrollTop).toBe(240);

        fixture.setHeight(2000);
        fixture.update([...history.slice(0, 2), makeMessage({ id: 5, message_type: "user" }),
            makeMessage({ id: 6, message_type: "butler_processing" })]);
        expect(bubble.mock.calls.map(call => call[1])).toEqual([5]);
        expect(fixture.container.scrollTop).toBe(240);
        act(() => { vi.advanceTimersByTime(700); });
        expect(fixture.container.scrollTop).toBe(1600);
        fixture.unmount();
    });

    it("should position asynchronously loaded history without treating it as a first send", () => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const slide = vi.spyOn(sendSlide, "startSendScrollSequence");
        const fixture = setup([], false);
        fixture.setHeight(1600);
        fixture.update([
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2, message_type: "response" }),
        ], { historyLoaded: true });
        act(() => { vi.advanceTimersByTime(500); });
        expect(fixture.container.scrollTop).toBe(1200);
        expect(bubble).not.toHaveBeenCalled();
        expect(slide).not.toHaveBeenCalled();
        fixture.unmount();
    });

    it("should open history after a conversation switch clears the list without a send animation", () => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const fixture = setup([makeMessage({ id: 1, message_type: "user" })]);
        fixture.update([], { conversationId: "2", historyLoaded: false });
        fixture.setHeight(2000);
        fixture.update(Array.from({ length: 120 }, (_, index) => makeMessage({
            id: index + 10, conversation_id: 2, message_type: index % 2 ? "response" : "user",
        })), { conversationId: "2", historyLoaded: true });
        act(() => { vi.advanceTimersByTime(500); });
        expect(fixture.container.scrollTop).toBe(1600);
        expect(bubble).not.toHaveBeenCalled();
        expect(fixture.container.querySelector('[data-aipp-initial-bottom-positioning="true"]')).toBeNull();
        fixture.unmount();
    });

    it("should animate the first actual send after an empty history finishes loading", () => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const fixture = setup([], false);
        fixture.update([], { historyLoaded: true });
        fixture.setHeight(1000);
        fixture.update([
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2, message_type: "butler_processing" }),
        ], { historyLoaded: true });
        expect(bubble.mock.calls.map(call => call[1])).toEqual([1]);
        expect(fixture.container.scrollTop).toBe(0);
        act(() => { vi.advanceTimersByTime(700); });
        expect(fixture.container.scrollTop).toBe(600);
        fixture.unmount();
    });

    it.each(["butler_processing", "response"])("should animate the first send when %s is batched after it", (type) => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const slide = vi.spyOn(sendSlide, "startSendScrollSequence");
        const fixture = setup();
        fixture.setHeight(1000);
        fixture.update([
            makeMessage({ id: 1, message_type: "user" }),
            makeMessage({ id: 2, message_type: type }),
        ]);
        expect(bubble).toHaveBeenCalledTimes(1);
        expect(bubble).toHaveBeenCalledWith(fixture.container, 1);
        expect(slide).toHaveBeenCalledWith(expect.objectContaining({ container: fixture.container, messageId: 1 }));
        expect(fixture.container.scrollTop).toBe(0);
        act(() => { vi.advanceTimersByTime(700); });
        expect(fixture.container.scrollTop).toBe(600);
        fixture.unmount();
    });

    it("should start a slide when a subpixel correction is followed by a large layout correction", () => {
        const slide = vi.spyOn(sendSlide, "startSendScrollSequence");
        const fixture = setup();
        fixture.setHeight(400.75);
        fixture.update([makeMessage({ id: 1, message_type: "user" })]);
        expect(slide).toHaveBeenCalledTimes(1);
        fixture.setHeight(1000);
        act(() => { vi.advanceTimersByTime(32); });
        expect(fixture.container.scrollTop).toBe(0);
        act(() => { vi.advanceTimersByTime(700); });
        expect(fixture.container.scrollTop).toBe(600);
        expect(slide).toHaveBeenCalledTimes(1);
        fixture.unmount();
    });

    it.each(["wheel", "touchmove"])("should cancel visual motion when the user interrupts with %s", (type) => {
        const cancel = vi.fn();
        vi.spyOn(sentBubble, "playSentBubbleAnimation").mockReturnValue(cancel);
        const fixture = setup();
        fixture.setHeight(1000);
        const messages = [makeMessage({ id: 1, message_type: "user" })];
        fixture.update(messages);
        act(() => { fixture.container.dispatchEvent(type === "wheel" ? new WheelEvent(type, { deltaY: -100 }) : new Event(type)); });
        expect(cancel).toHaveBeenCalledTimes(1);
        fixture.container.scrollTop = 200;
        fixture.setHeight(1200);
        fixture.update([...messages, makeMessage({ id: 2, message_type: "response" })]);
        act(() => { vi.advanceTimersByTime(500); });
        expect(fixture.container.scrollTop).toBe(200);
        fixture.unmount();
    });

    it("should animate a new turn in virtualized history but not replay the opened turn", () => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const history = Array.from({ length: 120 }, (_, index) => makeMessage({
            id: index + 1, message_type: index % 2 ? "response" : "user",
        }));
        const fixture = setup(history);
        act(() => { vi.advanceTimersByTime(500); });
        expect(bubble).not.toHaveBeenCalled();
        fixture.setHeight(1600);
        const messages = [...history, makeMessage({ id: 121, message_type: "user" }), makeMessage({ id: 122, message_type: "butler_processing" })];
        fixture.update(messages);
        expect(bubble.mock.calls.map(call => call[1])).toEqual([121]);
        fixture.update([...messages.slice(0, -1), makeMessage({ id: 123, message_type: "response" })]);
        expect(bubble).toHaveBeenCalledTimes(1);
        act(() => { vi.advanceTimersByTime(700); });
        expect(fixture.container.scrollTop).toBe(1200);
        fixture.unmount();
    });

    it("should follow delayed layout changes after the send pin settles", () => {
        const fixture = setup();
        fixture.setHeight(1000);
        fixture.update([makeMessage({ id: 1, message_type: "user" })]);
        act(() => { vi.advanceTimersByTime(1000); });
        fixture.setHeight(1400);
        act(() => { vi.advanceTimersByTime(200); });
        expect(fixture.container.scrollTop).toBe(1000);
        fixture.unmount();
    });

    it("should pin without visual animation when reduced motion is requested", () => {
        vi.stubGlobal("matchMedia", () => ({ matches: true }));
        const animate = vi.fn();
        const original = HTMLElement.prototype.animate;
        HTMLElement.prototype.animate = animate;
        const fixture = setup();
        fixture.setHeight(1000);
        fixture.update([makeMessage({ id: 1, message_type: "user" }), makeMessage({ id: 2, message_type: "butler_processing" })]);
        act(() => { vi.advanceTimersByTime(1000); });
        expect(fixture.container.scrollTop).toBe(600);
        expect(animate).not.toHaveBeenCalled();
        fixture.unmount();
        HTMLElement.prototype.animate = original;
    });

    it("should animate each new user once and cancel the previous send on conversation change", () => {
        const bubble = vi.spyOn(sentBubble, "playSentBubbleAnimation");
        const cancel = vi.fn();
        vi.spyOn(sentBubble, "playSentBubbleAnimation").mockReturnValue(cancel);
        const fixture = setup();
        fixture.setHeight(1000);
        const first = makeMessage({ id: 1, message_type: "user" });
        fixture.update([first]);
        fixture.setHeight(1600);
        fixture.update([first, makeMessage({ id: 2, message_type: "user" }), makeMessage({ id: 3, message_type: "butler_processing" })]);
        expect(bubble.mock.calls.map(call => call[1])).toEqual([1, 2]);
        expect(cancel).toHaveBeenCalledTimes(1);
        fixture.update([makeMessage({ id: 10, conversation_id: 2, message_type: "user" })], { conversationId: "2" });
        expect(cancel).toHaveBeenCalledTimes(2);
        expect(bubble).toHaveBeenCalledTimes(2);
        fixture.unmount();
    });
});
