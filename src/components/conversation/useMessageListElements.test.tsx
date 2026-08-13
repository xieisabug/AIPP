import { render, renderHook, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
    findFirstLiveSuffixIndex,
    useMessageListElements,
    type UseMessageListElementsProps,
} from "./useMessageListElements";
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
        ...makeHookProps(messages),
    });

    return (
        <>
            {messageElements.map(({ messageId, messageElement }) => (
                <div key={messageId}>{messageElement}</div>
            ))}
        </>
    );
}

function makeHookProps(
    messages: Message[],
): UseMessageListElementsProps {
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
    };
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

    it("renders a Butler user message followed by an error message", () => {
        render(
            <Harness
                messages={[
                    makeMessage({ id: 9985, message_type: "user", content: "你好" }),
                    makeMessage({
                        id: 9986,
                        message_type: "error",
                        content: "No available providers",
                    }),
                ]}
            />
        );

        expect(screen.getByTestId("message-9985")).toHaveTextContent("你好");
        expect(screen.getByTestId("message-9986")).toHaveTextContent("No available providers");
    });

    it("uses the whole merged group to estimate historical row height", () => {
        const mergedMessages = [
            makeMessage({ id: 1, message_type: "user", content: "user-1" }),
            makeMessage({
                id: 2,
                message_type: "response",
                content: Array.from({ length: 240 }, () => "a very long assistant response line").join("\n"),
            }),
            makeMessage({ id: 3, message_type: "tool_result", content: "ok" }),
            makeMessage({ id: 4, message_type: "user", content: "user-2" }),
        ];
        const singleMessageGroup = [
            makeMessage({ id: 1, message_type: "user", content: "user-1" }),
            makeMessage({ id: 3, message_type: "tool_result", content: "ok" }),
            makeMessage({ id: 4, message_type: "user", content: "user-2" }),
        ];

        const { result: mergedResult } = renderHook(() =>
            useMessageListElements(makeHookProps(mergedMessages)),
        );
        const { result: singleResult } = renderHook(() =>
            useMessageListElements(makeHookProps(singleMessageGroup)),
        );

        const mergedGroupItem = mergedResult.current.renderItems.find(
            (item) => item.key === "message-3",
        );
        const singleItem = singleResult.current.renderItems.find(
            (item) => item.key === "message-3",
        );

        expect(mergedGroupItem?.messageIds).toEqual([2, 3]);
        expect(mergedGroupItem?.estimatedHeight ?? 0).toBeGreaterThan(
            singleItem?.estimatedHeight ?? 0,
        );
    });

    it("caps historical row height estimates for auto-collapsed code blocks", () => {
        const longCodeBlock = [
            "```ts",
            ...Array.from({ length: 240 }, (_, index) => `const value${index} = ${index};`),
            "```",
        ].join("\n");
        const longPlainText = Array.from(
            { length: 240 },
            (_, index) => `plain response line ${index}`,
        ).join("\n");

        const { result: codeResult } = renderHook(() =>
            useMessageListElements(
                makeHookProps([
                    makeMessage({ id: 1, message_type: "user", content: "user-1" }),
                    makeMessage({ id: 2, message_type: "response", content: longCodeBlock }),
                    makeMessage({ id: 3, message_type: "user", content: "user-2" }),
                ]),
            ),
        );
        const { result: plainResult } = renderHook(() =>
            useMessageListElements(
                makeHookProps([
                    makeMessage({ id: 1, message_type: "user", content: "user-1" }),
                    makeMessage({ id: 2, message_type: "response", content: longPlainText }),
                    makeMessage({ id: 3, message_type: "user", content: "user-2" }),
                ]),
            ),
        );

        const codeItem = codeResult.current.renderItems.find(
            (item) => item.key === "message-2",
        );
        const plainItem = plainResult.current.renderItems.find(
            (item) => item.key === "message-2",
        );

        expect(codeItem?.estimatedHeight ?? 0).toBeLessThan(1500);
        expect(codeItem?.estimatedHeight ?? 0).toBeLessThan(
            plainItem?.estimatedHeight ?? 0,
        );
    });
});

describe("findFirstLiveSuffixIndex", () => {
    it("only treats the trailing live segment as non-virtualized", () => {
        expect(
            findFirstLiveSuffixIndex([
                { virtualizationMode: "virtualized" },
                { virtualizationMode: "live" },
                { virtualizationMode: "virtualized" },
                { virtualizationMode: "live" },
                { virtualizationMode: "live" },
            ]),
        ).toBe(3);
    });

    it("does not create a live suffix when no trailing item is live", () => {
        expect(
            findFirstLiveSuffixIndex([
                { virtualizationMode: "virtualized" },
                { virtualizationMode: "live" },
                { virtualizationMode: "virtualized" },
            ]),
        ).toBe(-1);
    });
});
