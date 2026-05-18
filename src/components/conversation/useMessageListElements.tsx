import React, { useMemo } from "react";

import MessageItem from "../MessageItem";
import VersionPagination from "../VersionPagination";
import { Message, StreamEvent } from "../../data/Conversation";
import type { InlineInteractionItem } from "../ConversationUI";
import { PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX } from "../../utils/previewCode";
import { messageContainsPreviewCode } from "@/utils/previewCodeDetection";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";
import { useFeishuDebugResend } from "@/hooks/useFeishuDebugResend";
import {
    LARGE_MESSAGE_PREVIEW_HEIGHT_ESTIMATE,
    shouldUseLargeMessagePreview,
} from "@/utils/largeMessagePreview";
import { ShineBorder } from "../magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import MessageActionButtons from "../message-item/MessageActionButtons";
import {
    LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX,
    LAST_REPLY_CONTAINER_MIN_HEIGHT,
} from "./layoutConstants";
import { findLastReplyStartIndex } from "./lastReplyLayout";

export interface UseMessageListElementsProps {
    allDisplayMessages: Message[];
    streamingMessages: Map<number, StreamEvent>;
    shiningMessageIds: Set<number>;
    shiningMcpCallId: number | null;
    reasoningExpandStates: Map<number, boolean>;
    mcpToolCallStates: Map<number, any>;
    generationGroups: Map<string, any>;
    selectedVersions: Map<string, number>;
    getGenerationGroupControl: (message: Message) => any;
    handleGenerationVersionChange: (groupId: string, versionIndex: number) => void;
    onCodeRun: (lang: string, inputStr: string) => void;
    onMessageRegenerate: (messageId: number) => void;
    onMessageEdit: (message: Message) => void;
    onMessageFork: (messageId: number) => void;
    onQueuedMessagePromote?: (queueId: number) => void;
    onToggleReasoningExpand: (messageId: number) => void;
    inlineInteractionItems?: InlineInteractionItem[];
    allowFeishuDebugResend?: boolean;
}

export interface MessageElementEntry {
    messageId: number;
    messageIds?: number[];
    estimatedHeight?: number;
    messageElement: React.ReactElement;
    groupControl: any;
}

export interface RenderableConversationItem {
    key: string;
    messageId: number | null;
    messageIds?: number[];
    estimatedHeight: number;
    element: React.ReactElement;
    virtualizationMode?: "virtualized" | "live";
}

export function findFirstLiveSuffixIndex(
    items: Pick<RenderableConversationItem, "virtualizationMode">[],
): number {
    let index = items.length;
    while (index > 0 && items[index - 1].virtualizationMode === "live") {
        index -= 1;
    }

    return index < items.length ? index : -1;
}

function stripMcpToolCallMarkup(content: string): string {
    return content
        .replace(/<!--\s*MCP_TOOL_CALL(?:_STREAMING)?[\s\S]*?-->/g, "")
        .replace(/<mcp_tool_call[\s\S]*?<\/mcp_tool_call>/gi, "")
        .trim();
}

function countMcpToolCalls(content: string): number {
    return (
        (content.match(/<!--\s*MCP_TOOL_CALL(?:_STREAMING)?[\s\S]*?-->/g) ?? []).length
        + (content.match(/<mcp_tool_call[\s\S]*?<\/mcp_tool_call>/gi) ?? []).length
    );
}

function countPreviewCodeToolCalls(content: string): number {
    return (
        (content.match(/"tool_name"\s*:\s*"preview_code"/g) ?? []).length
        + (content.match(/<tool_name>\s*preview_code\s*<\/tool_name>/gi) ?? []).length
    );
}

const CODE_BLOCK_HEIGHT_ESTIMATE_WRAP_CHARS = 96;
const CODE_BLOCK_COLLAPSED_VISIBLE_LINE_COUNT = 16;
const CODE_BLOCK_AUTO_COLLAPSE_LINE_THRESHOLD = 18;

function estimateWrappedCodeLineCount(code: string): number {
    return Math.max(
        1,
        code.split(/\r?\n/).reduce((total, line) => {
            return total + Math.max(
                1,
                Math.ceil(line.length / CODE_BLOCK_HEIGHT_ESTIMATE_WRAP_CHARS),
            );
        }, 0),
    );
}

function normalizeCollapsedCodeBlocksForHeightEstimate(content: string): string {
    return content.replace(
        /(^|\n)(`{3,}|~{3,})([^\n]*)\n([\s\S]*?)(?:\n\2(?=\n|$)|$)/g,
        (match, leading: string, fence: string, meta: string, code: string) => {
            const visualLineCount = estimateWrappedCodeLineCount(code);
            if (visualLineCount <= CODE_BLOCK_AUTO_COLLAPSE_LINE_THRESHOLD) {
                return match;
            }

            const visiblePlaceholder = Array.from(
                { length: CODE_BLOCK_COLLAPSED_VISIBLE_LINE_COUNT },
                () => "code",
            ).join("\n");

            return `${leading}${fence}${meta}\n${visiblePlaceholder}\n${fence}`;
        },
    );
}

function collectPreviewCodePayloadLengths(content: string): number[] {
    const segments = [
        ...(content.match(/<!--\s*MCP_TOOL_CALL(?:_STREAMING)?[\s\S]*?-->/g) ?? []),
        ...(content.match(/<mcp_tool_call[\s\S]*?<\/mcp_tool_call>/gi) ?? []),
    ];

    return segments
        .filter((segment) => messageContainsPreviewCode(segment))
        .map((segment) => segment.length);
}

function estimateMessageHeight(
    message: Message,
    options: { isLastMessage?: boolean; isReasoningExpanded?: boolean } = {},
): number {
    const { isLastMessage = false, isReasoningExpanded = false } = options;
    const rawContent = message.content ?? "";
    const content = normalizeCollapsedCodeBlocksForHeightEstimate(
        stripMcpToolCallMarkup(rawContent),
    );
    const mcpToolCallCount = countMcpToolCalls(rawContent);
    const previewCodePayloadLengths = rawContent.includes("MCP_TOOL_CALL_STREAMING")
        ? []
        : collectPreviewCodePayloadLengths(rawContent);
    const previewCodeToolCallCount = rawContent.includes("MCP_TOOL_CALL_STREAMING")
        ? 0
        : previewCodePayloadLengths.length || countPreviewCodeToolCalls(rawContent);
    const genericToolCallCount = Math.max(
        0,
        mcpToolCallCount - previewCodeToolCallCount,
    );
    const contentLength = content.length;
    const lineCount = content.length > 0
        ? content.split(/\r?\n/).length
        : 1;
    const wrappedLineCount = content.split(/\r?\n/).reduce(
        (total, line) => total + Math.max(0, Math.ceil(line.length / 96) - 1),
        0,
    );
    const codeBlockCount = Math.floor((content.match(/```/g) ?? []).length / 2);
    const listItemCount = (content.match(/^\s*(?:[-*+]|\d+\.)\s+/gm) ?? []).length;
    const tableRowCount = (content.match(/^\|.*\|$/gm) ?? []).length;
    const hasToolCall = mcpToolCallCount > 0;

    if (
        shouldUseLargeMessagePreview({
            content: rawContent,
            isLastMessage,
            isStreaming: false,
            messageType: message.message_type,
            previewMetadata: message.large_message_preview,
        })
    ) {
        return LARGE_MESSAGE_PREVIEW_HEIGHT_ESTIMATE;
    }

    const structureContributionCap =
        lineCount > 240 || contentLength > 6000 ? 12000 : 5200;
    const structureContribution = Math.min(
        structureContributionCap,
        Math.max(0, lineCount - 1) * 22
        + wrappedLineCount * 18
        + codeBlockCount * 240
        + listItemCount * 10
        + tableRowCount * 18,
    );
    const lengthContribution = Math.min(1600, Math.ceil(contentLength / 240) * 12);
    const previewCodeBaseHeight = isLastMessage
        ? PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX + 144
        : PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX + 120;
    const previewCodeContribution = previewCodeToolCallCount > 0
        ? previewCodePayloadLengths.length > 0
            ? previewCodePayloadLengths.reduce((sum, payloadLength) => {
                const payloadOverhead = isLastMessage
                    ? Math.min(40, Math.ceil(payloadLength / 6000) * 20)
                    : Math.min(24, Math.ceil(payloadLength / 6000) * 12);
                return sum + previewCodeBaseHeight + payloadOverhead;
            }, 0)
            : previewCodeToolCallCount * previewCodeBaseHeight
        : 0;
    const toolCallContribution = mcpToolCallCount > 0
        ? previewCodeContribution + genericToolCallCount * 88
        : 0;

    switch (message.message_type) {
        case "response":
            if (hasToolCall) {
                return 112 + structureContribution + lengthContribution + toolCallContribution;
            }
            return 180 + structureContribution + lengthContribution + toolCallContribution;
        case "tool_result":
            return 160 + structureContribution + lengthContribution + toolCallContribution;
        case "reasoning":
            if (!isReasoningExpanded) {
                return hasToolCall ? 140 : 72;
            }
            return 132 + structureContribution + lengthContribution + toolCallContribution;
        case "user":
            return 100
                + Math.min(1800, Math.max(0, lineCount - 1) * 14 + wrappedLineCount * 10)
                + Math.min(320, Math.ceil(contentLength / 320) * 14);
        case "system":
            return 96
                + Math.min(960, Math.max(0, lineCount - 1) * 12 + wrappedLineCount * 8)
                + Math.min(240, Math.ceil(contentLength / 480) * 12);
        default:
            return 120 + structureContribution + lengthContribution + toolCallContribution;
    }
}

export function useMessageListElements({
    allDisplayMessages,
    streamingMessages,
    shiningMessageIds,
    shiningMcpCallId,
    reasoningExpandStates,
    mcpToolCallStates,
    generationGroups,
    selectedVersions,
    getGenerationGroupControl,
    handleGenerationVersionChange,
    onCodeRun,
    onMessageRegenerate,
    onMessageEdit,
    onMessageFork,
    onQueuedMessagePromote,
    onToggleReasoningExpand,
    inlineInteractionItems,
    allowFeishuDebugResend = false,
}: UseMessageListElementsProps) {
    const { isMergeAssistantMessages } = useDisplayConfig();
    const { pendingMessageId, resendMessageToFeishuDebug } = useFeishuDebugResend();

    const messageInlineInteractionMap = useMemo(() => {
        const map = new Map<number, InlineInteractionItem[]>();
        (inlineInteractionItems ?? []).forEach((item) => {
            if (item.messageId === undefined || item.messageId === null) {
                return;
            }
            const existing = map.get(item.messageId) ?? [];
            map.set(item.messageId, [...existing, item]);
        });
        return map;
    }, [inlineInteractionItems]);

    const displayedMessageIdSet = useMemo(
        () => new Set(allDisplayMessages.map((message) => message.id)),
        [allDisplayMessages],
    );

    const fallbackInlineInteractionItems = useMemo(() => {
        return (inlineInteractionItems ?? []).filter(
            (item) =>
                item.messageId === undefined
                || item.messageId === null
                || !displayedMessageIdSet.has(item.messageId),
        );
    }, [inlineInteractionItems, displayedMessageIdSet]);

    const messageById = useMemo(() => {
        return new Map(
            allDisplayMessages.map((message) => [message.id, message] as const),
        );
    }, [allDisplayMessages]);
    const lastMessageId = useMemo(
        () =>
            allDisplayMessages.length > 0
                ? allDisplayMessages[allDisplayMessages.length - 1].id
                : null,
        [allDisplayMessages],
    );
    const estimatedHeightByMessageId = useMemo(() => {
        return new Map(
            allDisplayMessages.map((message) => [
                message.id,
                estimateMessageHeight(message, {
                    isLastMessage: message.id === lastMessageId,
                    isReasoningExpanded:
                        reasoningExpandStates.get(message.id) || false,
                }),
            ] as const),
        );
    }, [allDisplayMessages, lastMessageId, reasoningExpandStates]);

    const messageElements = useMemo(() => {
        if (!isMergeAssistantMessages) {
            // 非合并模式：每条消息单独渲染
            return allDisplayMessages.map((message) => {
                const streamEvent = streamingMessages.get(message.id);
                const groupControl = getGenerationGroupControl(message);
                const shouldShowShineBorder = shiningMessageIds.has(message.id);
                const isLastMessage = message.id === lastMessageId;

                return {
                    messageId: message.id,
                    messageIds: [message.id],
                    estimatedHeight:
                        estimatedHeightByMessageId.get(message.id)
                        ?? estimateMessageHeight(message, {
                            isLastMessage: message.id === lastMessageId,
                            isReasoningExpanded:
                                reasoningExpandStates.get(message.id) || false,
                        }),
                    messageElement: (
                        <MessageItem
                            key={`message-${message.id}`}
                            message={message}
                            streamEvent={streamEvent}
                            onCodeRun={onCodeRun}
                            onMessageRegenerate={() => onMessageRegenerate(message.id)}
                            onMessageEdit={() => onMessageEdit(message)}
                            onMessageFork={() => onMessageFork(message.id)}
                            onQueuedMessagePromote={onQueuedMessagePromote}
                            isReasoningExpanded={
                                reasoningExpandStates.get(message.id) || false
                            }
                            onToggleReasoningExpand={() =>
                                onToggleReasoningExpand(message.id)
                            }
                            shouldShowShineBorder={shouldShowShineBorder}
                            conversationId={message.conversation_id}
                            mcpToolCallStates={mcpToolCallStates}
                            shiningMcpCallId={shiningMcpCallId}
                            isLastMessage={isLastMessage}
                            inlineInteractionItems={messageInlineInteractionMap.get(
                                message.id,
                            )}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                        />
                    ),
                    groupControl,
                } satisfies MessageElementEntry;
            });
        }

        // 合并模式：将连续的非 user 消息合并到一个气泡中
        const result: MessageElementEntry[] = [];
        let currentGroup: Message[] = [];

        const flushGroup = (options?: { keepExpanded?: boolean }) => {
            if (currentGroup.length === 0) return;

            if (currentGroup.length === 1) {
                // 单条消息也用合并气泡包裹（保持一致性）
            }

            const groupMessages = [...currentGroup];
            const lastMsg = groupMessages[groupMessages.length - 1];
            const keepExpanded = options?.keepExpanded ?? false;
            // 合并组使用最后一条消息的 groupControl
            const groupControl = getGenerationGroupControl(lastMsg);
            const anyShining = groupMessages.some((m) => shiningMessageIds.has(m.id));

            const mergedElement = (
                <div
                    key={`merged-${groupMessages.map((m) => m.id).join("-")}`}
                    className="flex flex-col"
                    data-message-item
                    data-message-id={lastMsg.id}
                    data-message-type="merged"
                >
                    <div className="group relative py-4 px-5 rounded-2xl inline-block max-w-[65%] transition-all duration-200 bg-background text-foreground border border-border self-start">
                        {anyShining && (
                            <ShineBorder
                                shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                                borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                                duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                            />
                        )}
                        <div className="flex flex-col gap-2">
                            {groupMessages.map((message) => {
                                const streamEvent = streamingMessages.get(message.id);
                                const isLast = keepExpanded || message.id === lastMessageId;

                                return (
                                    <MessageItem
                                        key={`message-${message.id}`}
                                        message={message}
                                        streamEvent={streamEvent}
                                        onCodeRun={onCodeRun}
                                        onMessageRegenerate={() => onMessageRegenerate(message.id)}
                                        onMessageEdit={() => onMessageEdit(message)}
                                        onMessageFork={() => onMessageFork(message.id)}
                                        onQueuedMessagePromote={onQueuedMessagePromote}
                                        isReasoningExpanded={
                                            reasoningExpandStates.get(message.id) || false
                                        }
                                        onToggleReasoningExpand={() =>
                                            onToggleReasoningExpand(message.id)
                                        }
                                        shouldShowShineBorder={false}
                                        conversationId={message.conversation_id}
                                        mcpToolCallStates={mcpToolCallStates}
                                        shiningMcpCallId={shiningMcpCallId}
                                        isLastMessage={isLast}
                                        inlineInteractionItems={messageInlineInteractionMap.get(
                                            message.id,
                                        )}
                                        allowFeishuDebugResend={allowFeishuDebugResend}
                                        mergedMode
                                    />
                                );
                            })}
                        </div>
                        {(() => {
                            const lastResponse = [...groupMessages].reverse().find((m) => m.message_type === "response");
                            const resendTargetMessage = [...groupMessages]
                                .reverse()
                                .find((m) => m.message_type === "response" || m.message_type === "tool_result");
                            const toolbarMessage = lastResponse ?? resendTargetMessage;

                            if (!toolbarMessage) return null;

                            return (
                                <MessageActionButtons
                                    messageId={toolbarMessage.id}
                                    messageType={lastResponse ? "response" : toolbarMessage.message_type}
                                    isUserMessage={false}
                                    copyIconState="copy"
                                    onCopy={() => {
                                        const responseContent = groupMessages
                                            .filter((m) => m.message_type === "response")
                                            .map((m) => m.content);
                                        const content = responseContent.length > 0
                                            ? responseContent.join("\n")
                                            : toolbarMessage.content;
                                        navigator.clipboard.writeText(content);
                                    }}
                                    onRegenerate={lastResponse ? () => onMessageRegenerate(lastResponse.id) : undefined}
                                    onFork={lastResponse ? () => onMessageFork(lastResponse.id) : undefined}
                                    onResendToFeishuDebug={
                                        allowFeishuDebugResend && resendTargetMessage
                                            ? () => void resendMessageToFeishuDebug(resendTargetMessage.id)
                                            : undefined
                                    }
                                    isResendToFeishuDebugPending={pendingMessageId === resendTargetMessage?.id}
                                    messageContent={toolbarMessage.content}
                                />
                            );
                        })()}
                    </div>
                </div>
            );

            result.push({
                messageId: lastMsg.id,
                messageIds: groupMessages.map((message) => message.id),
                estimatedHeight: groupMessages.reduce((sum, message) => {
                    return sum + (
                        estimatedHeightByMessageId.get(message.id)
                        ?? estimateMessageHeight(message, {
                            isLastMessage: message.id === lastMessageId,
                            isReasoningExpanded:
                                reasoningExpandStates.get(message.id) || false,
                        })
                    );
                }, 0),
                messageElement: mergedElement,
                groupControl,
            });

            currentGroup = [];
        };

        for (const message of allDisplayMessages) {
            if (message.message_type === "user") {
                flushGroup({ keepExpanded: false });
                const streamEvent = streamingMessages.get(message.id);
                const groupControl = getGenerationGroupControl(message);
                const shouldShowShineBorder = shiningMessageIds.has(message.id);
                const isLastMessage = message.id === lastMessageId;

                result.push({
                    messageId: message.id,
                    messageElement: (
                        <MessageItem
                            key={`message-${message.id}`}
                            message={message}
                            streamEvent={streamEvent}
                            onCodeRun={onCodeRun}
                            onMessageRegenerate={() => onMessageRegenerate(message.id)}
                            onMessageEdit={() => onMessageEdit(message)}
                            onMessageFork={() => onMessageFork(message.id)}
                            onQueuedMessagePromote={onQueuedMessagePromote}
                            isReasoningExpanded={
                                reasoningExpandStates.get(message.id) || false
                            }
                            onToggleReasoningExpand={() =>
                                onToggleReasoningExpand(message.id)
                            }
                            shouldShowShineBorder={shouldShowShineBorder}
                            conversationId={message.conversation_id}
                            mcpToolCallStates={mcpToolCallStates}
                            shiningMcpCallId={shiningMcpCallId}
                            isLastMessage={isLastMessage}
                            inlineInteractionItems={messageInlineInteractionMap.get(
                                message.id,
                            )}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                        />
                    ),
                    groupControl,
                });
            } else {
                currentGroup.push(message);
            }
        }

        flushGroup({ keepExpanded: true });
        return result;
    }, [
        allDisplayMessages,
        streamingMessages,
        getGenerationGroupControl,
        shiningMessageIds,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onMessageFork,
        onQueuedMessagePromote,
        reasoningExpandStates,
        onToggleReasoningExpand,
        mcpToolCallStates,
        shiningMcpCallId,
        messageInlineInteractionMap,
        estimatedHeightByMessageId,
        allowFeishuDebugResend,
        lastMessageId,
        isMergeAssistantMessages,
        pendingMessageId,
        resendMessageToFeishuDebug,
    ]);

    const versionControlElements = useMemo(() => {
        return messageElements
            .filter(({ groupControl }) => groupControl)
            .map(({ messageId, groupControl }) => (
                <div key={`version-${messageId}`} className="flex justify-start mt-2">
                    <VersionPagination
                        currentVersion={groupControl.currentVersion}
                        totalVersions={groupControl.totalVersions}
                        onVersionChange={(versionIndex) =>
                            handleGenerationVersionChange(
                                groupControl.groupId,
                                versionIndex,
                            )
                        }
                    />
                </div>
            ));
    }, [messageElements, handleGenerationVersionChange]);

    const versionMap = useMemo(() => {
        const map = new Map<string, React.ReactElement>();
        versionControlElements.forEach((element) => {
            const key = element.key != null ? String(element.key) : "";
            if (key) {
                map.set(key, element);
            }
        });
        return map;
    }, [versionControlElements]);

    const placeholderElements = useMemo(() => {
        const placeholders: React.ReactElement[] = [];

        generationGroups.forEach((group, groupId) => {
            const selectedVersionIndex =
                selectedVersions.get(groupId)
                ?? (group.versions.length > 0 ? group.versions.length - 1 : 0);
            const selectedVersionData = group.versions[selectedVersionIndex];

            if (selectedVersionData?.isPlaceholder) {
                placeholders.push(
                    <React.Fragment key={`placeholder_${groupId}`}>
                        <div className="flex justify-start mb-4">
                            <div className="bg-muted rounded-lg p-4 max-w-3xl">
                                <div className="flex items-center space-x-2">
                                    <div className="animate-spin rounded-full h-4 w-4 border-b-2 border-foreground"></div>
                                    <span className="text-sm text-muted-foreground">
                                        正在重新生成...
                                    </span>
                                </div>
                            </div>
                        </div>
                        <div className="flex justify-start mt-2">
                            <VersionPagination
                                currentVersion={selectedVersionIndex + 1}
                                totalVersions={group.versions.length}
                                onVersionChange={(versionIndex) =>
                                    handleGenerationVersionChange(
                                        groupId,
                                        versionIndex,
                                    )
                                }
                            />
                        </div>
                    </React.Fragment>,
                );
            }
        });

        return placeholders;
    }, [generationGroups, selectedVersions, handleGenerationVersionChange]);

    const renderItems = useMemo(() => {
        const items: RenderableConversationItem[] = [];
        const lastReplyStartIndex = findLastReplyStartIndex(
            allDisplayMessages,
            messageElements,
        );

        const pushMessageWithVersion = (entry: MessageElementEntry) => {
            const sourceMessage = messageById.get(entry.messageId) ?? allDisplayMessages[0];
            const shouldKeepLive =
                sourceMessage?.message_type === "reasoning"
                || countPreviewCodeToolCalls(sourceMessage?.content ?? "") > 0;
            items.push({
                key: `message-${entry.messageId}`,
                messageId: entry.messageId,
                messageIds: entry.messageIds ?? [entry.messageId],
                estimatedHeight:
                entry.estimatedHeight
                    ?? estimateMessageHeight(
                        messageById.get(entry.messageId) ?? allDisplayMessages[0],
                        {
                            isLastMessage: entry.messageId === lastMessageId,
                            isReasoningExpanded:
                                reasoningExpandStates.get(entry.messageId) || false,
                        },
                    ),
                element: entry.messageElement,
                virtualizationMode: shouldKeepLive ? "live" : "virtualized",
            });

            const versionElement = versionMap.get(`version-${entry.messageId}`);
            if (versionElement) {
                items.push({
                    key: `version-${entry.messageId}`,
                    messageId: null,
                    estimatedHeight: 48,
                    element: versionElement,
                    virtualizationMode: shouldKeepLive ? "live" : "virtualized",
                });
            }
        };

        if (lastReplyStartIndex >= 0) {
            const before = messageElements.slice(0, lastReplyStartIndex);
            const lastGroup = messageElements.slice(lastReplyStartIndex);

            before.forEach((entry) => {
                pushMessageWithVersion(entry);
            });

            const lastGroupMessageIds = lastGroup.map((entry) => entry.messageId);
            const lastGroupEstimatedHeight = Math.max(
                0,
                lastGroup.reduce((sum, entry) => {
                    let next = sum + (
                        estimatedHeightByMessageId.get(entry.messageId)
                        ?? estimateMessageHeight(
                            messageById.get(entry.messageId) ?? allDisplayMessages[0],
                            {
                                isLastMessage: entry.messageId === lastMessageId,
                                isReasoningExpanded:
                                    reasoningExpandStates.get(entry.messageId) || false,
                            },
                        )
                    );
                    if (versionMap.has(`version-${entry.messageId}`)) {
                        next += 48;
                    }
                    return next;
                }, 0)
                    + placeholderElements.length * 96
                    + fallbackInlineInteractionItems.length * 112
                    + 120,
            );

            items.push({
                key: "last-reply-container",
                messageId:
                    lastGroupMessageIds[lastGroupMessageIds.length - 1] ?? null,
                messageIds: lastGroupMessageIds,
                estimatedHeight: lastGroupEstimatedHeight,
                virtualizationMode: "live",
                element: (
                    <div
                        id="last-reply-container"
                        style={{ minHeight: LAST_REPLY_CONTAINER_MIN_HEIGHT }}
                        className="flex flex-col gap-4"
                        data-aipp-slot="chat-last-reply-container"
                    >
                        {lastGroup.map((entry) => (
                            <React.Fragment
                                key={`last-group-${entry.messageId}`}
                            >
                                {entry.messageElement}
                                {versionMap.get(`version-${entry.messageId}`)
                                    || null}
                            </React.Fragment>
                        ))}
                        {placeholderElements}
                        {fallbackInlineInteractionItems.length > 0 && (
                            <div className="flex flex-col gap-4 pt-2">
                                {fallbackInlineInteractionItems.map((item) => (
                                    <React.Fragment key={item.key}>
                                        {item.content}
                                    </React.Fragment>
                                ))}
                            </div>
                        )}
                        <div
                            className="flex-none"
                            style={{
                                height: LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX,
                            }}
                            data-aipp-slot="chat-bottom-spacer"
                        />
                    </div>
                ),
            });
        } else {
            placeholderElements.forEach((element, index) => {
                items.push({
                    key: `placeholder-${index}`,
                    messageId: null,
                    estimatedHeight: 96,
                    element,
                    virtualizationMode: "live",
                });
            });

            fallbackInlineInteractionItems.forEach((item, index) => {
                items.push({
                    key: `inline-${item.key}-${index}`,
                    messageId: item.messageId ?? null,
                    estimatedHeight: 112,
                    element: (
                        <div className="flex flex-col gap-4 pt-2">
                            <React.Fragment key={item.key}>
                                {item.content}
                            </React.Fragment>
                        </div>
                    ),
                    virtualizationMode: "live",
                });
            });
        }

        return items;
    }, [
        messageElements,
        versionMap,
        placeholderElements,
        fallbackInlineInteractionItems,
        allDisplayMessages,
        estimatedHeightByMessageId,
        messageById,
    ]);

    return {
        fallbackInlineInteractionItems,
        messageElements,
        placeholderElements,
        renderItems,
        versionMap,
    };
}
