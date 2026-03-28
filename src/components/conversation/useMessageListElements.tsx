import React, { useMemo } from "react";

import MessageItem from "../MessageItem";
import VersionPagination from "../VersionPagination";
import { Message, StreamEvent } from "../../data/Conversation";
import type { InlineInteractionItem } from "../ConversationUI";

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
    onToggleReasoningExpand: (messageId: number) => void;
    inlineInteractionItems?: InlineInteractionItem[];
    sentBatchToolResultMessageIds?: ReadonlySet<number>;
    allowFeishuDebugResend?: boolean;
}

export interface MessageElementEntry {
    messageId: number;
    messageElement: React.ReactElement;
    groupControl: any;
}

export interface RenderableConversationItem {
    key: string;
    messageId: number | null;
    messageIds?: number[];
    estimatedHeight: number;
    element: React.ReactElement;
}

function estimateMessageHeight(message: Message): number {
    const contentLength = message.content?.length ?? 0;
    const lengthContribution = Math.min(720, Math.ceil(contentLength / 240) * 28);

    switch (message.message_type) {
        case "response":
            return 180 + lengthContribution;
        case "tool_result":
            return 160 + lengthContribution;
        case "user":
            return 100 + Math.min(220, Math.ceil(contentLength / 320) * 18);
        case "system":
            return 96 + Math.min(120, Math.ceil(contentLength / 480) * 16);
        default:
            return 120 + lengthContribution;
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
    onToggleReasoningExpand,
    inlineInteractionItems,
    sentBatchToolResultMessageIds,
    allowFeishuDebugResend = false,
}: UseMessageListElementsProps) {
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

    const messageElements = useMemo(() => {
        const lastMessageId =
            allDisplayMessages.length > 0
                ? allDisplayMessages[allDisplayMessages.length - 1].id
                : -1;

        return allDisplayMessages.map((message) => {
            const streamEvent = streamingMessages.get(message.id);
            const groupControl = getGenerationGroupControl(message);
            const shouldShowShineBorder = shiningMessageIds.has(message.id);
            const isLastMessage = message.id === lastMessageId;

            return {
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
                        sentBatchToolResultMessageIds={
                            sentBatchToolResultMessageIds
                        }
                        allowFeishuDebugResend={allowFeishuDebugResend}
                    />
                ),
                groupControl,
            } satisfies MessageElementEntry;
        });
    }, [
        allDisplayMessages,
        streamingMessages,
        getGenerationGroupControl,
        shiningMessageIds,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onMessageFork,
        reasoningExpandStates,
        onToggleReasoningExpand,
        mcpToolCallStates,
        shiningMcpCallId,
        messageInlineInteractionMap,
        sentBatchToolResultMessageIds,
        allowFeishuDebugResend,
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
        let lastUserMessageIndex = -1;

        for (let i = allDisplayMessages.length - 1; i >= 0; i -= 1) {
            if (allDisplayMessages[i].message_type === "user") {
                lastUserMessageIndex = i;
                break;
            }
        }

        const pushMessageWithVersion = (entry: MessageElementEntry) => {
            items.push({
                key: `message-${entry.messageId}`,
                messageId: entry.messageId,
                messageIds: [entry.messageId],
                estimatedHeight: estimateMessageHeight(
                    messageById.get(entry.messageId) ?? allDisplayMessages[0],
                ),
                element: entry.messageElement,
            });

            const versionElement = versionMap.get(`version-${entry.messageId}`);
            if (versionElement) {
                items.push({
                    key: `version-${entry.messageId}`,
                    messageId: null,
                    estimatedHeight: 48,
                    element: versionElement,
                });
            }
        };

        if (lastUserMessageIndex >= 0) {
            const before = messageElements.slice(0, lastUserMessageIndex);
            const lastGroup = messageElements.slice(lastUserMessageIndex);

            before.forEach((entry) => {
                pushMessageWithVersion(entry);
            });

            const lastGroupMessageIds = lastGroup.map((entry) => entry.messageId);
            const lastGroupEstimatedHeight = Math.max(
                0,
                lastGroup.reduce((sum, entry) => {
                    let next = sum + estimateMessageHeight(
                        messageById.get(entry.messageId) ?? allDisplayMessages[0],
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
                estimatedHeight: Math.max(420, lastGroupEstimatedHeight),
                element: (
                    <div
                        id="last-reply-container"
                        style={{ minHeight: "calc(100dvh - 130px)" }}
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
                            className="flex-none h-[120px]"
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
