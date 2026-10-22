import React, { useMemo, useRef } from "react";

import MessageItem from "../MessageItem";
import VersionPagination from "../VersionPagination";
import { AgentActivityEvent, Message, StreamEvent } from "../../data/Conversation";
import { isHiddenAgentActivity } from "./AgentActivityList";
import type { InlineInteractionItem } from "../ConversationUI";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";
import { useFeishuDebugResend } from "@/hooks/useFeishuDebugResend";
import MergedAssistantGroup, {
    MergedAssistantGroupSegment,
    type MergedAssistantGroupProps,
} from "./MergedAssistantGroup";
import { splitMergedGroupMessages } from "./mergedGroupSegments";
import {
    LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX,
    LAST_REPLY_CONTAINER_MIN_HEIGHT,
} from "./layoutConstants";
import { findLastReplyStartIndex } from "./lastReplyLayout";
import {
    estimateMergedGroupHeight,
    estimateMergedSegmentHeight,
    estimateMessageHeight,
} from "./messageHeightEstimate";

export interface UseMessageListElementsProps {
    allDisplayMessages: Message[];
    streamingMessages: Map<number, StreamEvent>;
    shiningMessageIds: Set<number>;
    shiningMcpCallId: number | null;
    reasoningExpandStates: Map<number, boolean>;
    mcpToolCallStates: Map<number, any>;
    agentActivities?: Map<string, AgentActivityEvent>;
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
    renderMessageActions?: (message: Message) => React.ReactNode;
    actionableAgentPlanMessageId?: number | null;
    agentPlanModeSwitching?: boolean;
    onContinueAgentPlan?: () => void;
    onApproveAgentPlan?: () => void;
    /** Width of the message list, used to estimate text wrapping. */
    listWidthPx?: number;
}

export interface MessageElementSegment {
    key: string;
    messageIds: number[];
    estimatedHeight: number;
    element: React.ReactElement;
}

export interface MessageElementEntry {
    messageId: number;
    messageIds?: number[];
    estimatedHeight?: number;
    messageElement: React.ReactElement;
    /** Row-sized pieces of a tall merged bubble, used when it is virtualized as history. */
    segments?: MessageElementSegment[];
    groupControl: any;
}

export interface RenderableConversationItem {
    key: string;
    messageId: number | null;
    messageIds?: number[];
    estimatedHeight: number;
    element: React.ReactElement;
    virtualizationMode?: "virtualized" | "live";
    /** The next item continues the same bubble, so no row gap goes between them. */
    joinsNext?: boolean;
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

function countPreviewCodeToolCalls(content: string): number {
    return (
        (content.match(/"tool_name"\s*:\s*"preview_code"/g) ?? []).length
        + (content.match(/<tool_name>\s*preview_code\s*<\/tool_name>/gi) ?? []).length
    );
}

interface McpCallRefs {
    ids: number[];
    /** 存在没有数字 call_id 的工具调用，卡片要靠 llm_call_id 或自建的 call 查状态。 */
    needsFullMap: boolean;
}

const EMPTY_MCP_CALL_REFS: McpCallRefs = { ids: [], needsFullMap: false };
const mcpCallRefsCache = new WeakMap<Message, McpCallRefs>();
const previewCodeCountCache = new WeakMap<Message, number>();

export function readMcpCallRefs(message: Message): McpCallRefs {
    const cached = mcpCallRefsCache.get(message);
    if (cached) {
        return cached;
    }
    const content = message.content ?? "";
    let refs = EMPTY_MCP_CALL_REFS;
    if (content.includes("MCP_TOOL_CALL") || /<mcp_tool_call/i.test(content)) {
        const ids: number[] = [];
        let needsFullMap = /<mcp_tool_call/i.test(content);
        const markerPattern = /<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\s*:?/g;
        let marker: RegExpExecArray | null;
        while ((marker = markerPattern.exec(content)) !== null) {
            const payloadStart = marker.index + marker[0].length;
            const payloadEnd = content.indexOf("-->", payloadStart);
            const payload = content.slice(
                payloadStart,
                payloadEnd === -1 ? content.length : payloadEnd,
            );
            const idMatch = /^\s*(\d+)\s*$/.exec(payload)
                ?? /"call_id"\s*:\s*"?(\d+)/.exec(payload);
            const id = idMatch ? Number(idMatch[1]) : Number.NaN;
            if (Number.isFinite(id)) {
                ids.push(id);
            } else {
                needsFullMap = true;
            }
        }
        refs = { ids, needsFullMap };
    }
    mcpCallRefsCache.set(message, refs);
    return refs;
}

function cachedPreviewCodeCount(message: Message | undefined): number {
    if (!message) {
        return 0;
    }
    const cached = previewCodeCountCache.get(message);
    if (cached !== undefined) {
        return cached;
    }
    const count = countPreviewCodeToolCalls(message.content ?? "");
    previewCodeCountCache.set(message, count);
    return count;
}

function mcpEventUnchanged(previous: any, next: any): boolean {
    if (previous === next) {
        return true;
    }
    if (!previous || !next) {
        return false;
    }
    return previous.status === next.status
        && previous.error === next.error
        && previous.result === next.result
        && previous.parameters === next.parameters
        && previous.tool_name === next.tool_name;
}

function reuseMcpSlice(
    previous: Map<number, any> | undefined,
    ids: number[],
    source: Map<number, any>,
): Map<number, any> | undefined {
    const presentIds = ids.filter((id) => source.has(id));
    if (presentIds.length === 0) {
        return undefined;
    }
    if (
        previous
        && previous.size === presentIds.length
        && presentIds.every((id) => mcpEventUnchanged(previous.get(id), source.get(id)))
    ) {
        return previous;
    }
    const next = new Map<number, any>();
    for (const id of presentIds) {
        const incoming = source.get(id);
        const previousValue = previous?.get(id);
        next.set(
            id,
            previousValue && mcpEventUnchanged(previousValue, incoming) ? previousValue : incoming,
        );
    }
    return next;
}

export function useMessageListElements({
    allDisplayMessages,
    agentActivities = new Map(),
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
    renderMessageActions,
    actionableAgentPlanMessageId = null,
    agentPlanModeSwitching = false,
    onContinueAgentPlan,
    onApproveAgentPlan,
    listWidthPx,
}: UseMessageListElementsProps) {
    const { isMergeAssistantMessages, isShowThinking } = useDisplayConfig();
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

    // Agent 活动按所属响应消息分组，收进对应消息气泡内展示；
    // userMessage/agentMessage 这类已在正文体现的 kind 直接过滤掉（含历史数据）
    const previousActivitiesRef = useRef<Map<number, AgentActivityEvent[]>>(new Map());
    const activitiesByMessageId = useMemo(() => {
        const grouped = new Map<number, AgentActivityEvent[]>();
        agentActivities.forEach((activity) => {
            if (isHiddenAgentActivity(activity)) return;
            const existing = grouped.get(activity.response_message_id);
            if (existing) {
                existing.push(activity);
            } else {
                grouped.set(activity.response_message_id, [activity]);
            }
        });

        const previous = previousActivitiesRef.current;
        let unchanged = grouped.size === previous.size;
        grouped.forEach((list, messageId) => {
            const previousList = previous.get(messageId);
            if (
                previousList
                && previousList.length === list.length
                && previousList.every((activity, index) => activity === list[index])
            ) {
                grouped.set(messageId, previousList);
            } else {
                unchanged = false;
            }
        });
        if (unchanged) {
            return previous;
        }
        previousActivitiesRef.current = grouped;
        return grouped;
    }, [agentActivities]);
    const previousMcpSlicesRef = useRef<Map<number, Map<number, any> | undefined>>(new Map());
    const mcpSliceByMessageId = useMemo(() => {
        const previous = previousMcpSlicesRef.current;
        const next = new Map<number, Map<number, any> | undefined>();
        let unchanged = previous.size === allDisplayMessages.length;
        for (const message of allDisplayMessages) {
            const refs = readMcpCallRefs(message);
            const slice = refs.needsFullMap
                ? mcpToolCallStates
                : reuseMcpSlice(previous.get(message.id), refs.ids, mcpToolCallStates);
            next.set(message.id, slice);
            if (!previous.has(message.id) || previous.get(message.id) !== slice) {
                unchanged = false;
            }
        }
        if (unchanged) {
            return previous;
        }
        previousMcpSlicesRef.current = next;
        return next;
    }, [allDisplayMessages, mcpToolCallStates]);

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
    // Bucketed so window resizes do not re-estimate every message per pixel.
    const estimateListWidthPx = listWidthPx && listWidthPx > 0
        ? Math.round(listWidthPx / 40) * 40
        : undefined;
    // 高度估算按消息缓存：流式输出时只有尾部消息内容在变，历史消息命中缓存，
    // 避免每个 token 都对全部消息重跑重量级正则计算。
    const estimatedHeightCacheRef = useRef<{
        layoutKey: string;
        entries: Map<number, {
            content: string;
            messageType: string;
            finishTime: Message["finish_time"];
            isLast: boolean;
            expanded: boolean;
            activityCount: number;
            height: number;
        }>;
    }>({ layoutKey: "", entries: new Map() });
    const estimatedHeightByMessageId = useMemo(() => {
        const layoutKey = `${isMergeAssistantMessages}:${isShowThinking}:${estimateListWidthPx ?? ""}`;
        const previousCache = estimatedHeightCacheRef.current.layoutKey === layoutKey
            ? estimatedHeightCacheRef.current.entries
            : new Map();
        const nextCache: typeof estimatedHeightCacheRef.current.entries = new Map();
        const result = new Map<number, number>();

        for (const message of allDisplayMessages) {
            const content = message.content ?? "";
            const isLast = message.id === lastMessageId;
            const expanded = reasoningExpandStates.get(message.id) || false;
            const activityCount = activitiesByMessageId.get(message.id)?.length ?? 0;
            const cached = previousCache.get(message.id);

            let height: number;
            if (
                cached
                && cached.content === content
                && cached.messageType === message.message_type
                && cached.finishTime === message.finish_time
                && cached.isLast === isLast
                && cached.expanded === expanded
                && cached.activityCount === activityCount
            ) {
                height = cached.height;
            } else {
                height = estimateMessageHeight(message, {
                    isLastMessage: isLast,
                    isReasoningExpanded: expanded,
                    agentActivityCount: activityCount,
                    mergedMode: isMergeAssistantMessages
                        && message.message_type !== "user"
                        && message.message_type !== "agent_plan",
                    isShowThinking,
                    listWidthPx: estimateListWidthPx,
                });
            }

            result.set(message.id, height);
            nextCache.set(message.id, {
                content,
                messageType: message.message_type,
                finishTime: message.finish_time,
                isLast,
                expanded,
                activityCount,
                height,
            });
        }

        estimatedHeightCacheRef.current = { layoutKey, entries: nextCache };
        return result;
    }, [
        allDisplayMessages,
        lastMessageId,
        reasoningExpandStates,
        activitiesByMessageId,
        isMergeAssistantMessages,
        isShowThinking,
        estimateListWidthPx,
    ]);

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
                    estimatedHeight: estimatedHeightByMessageId.get(message.id) ?? 0,
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
                            mcpToolCallStates={mcpSliceByMessageId.get(message.id)}
                            shiningMcpCallId={shiningMcpCallId}
                            isLastMessage={isLastMessage}
                            inlineInteractionItems={messageInlineInteractionMap.get(
                                message.id,
                            )}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                            messageActions={renderMessageActions?.(message)}
                            agentActivities={activitiesByMessageId.get(message.id)}
                            canActOnAgentPlan={message.id === actionableAgentPlanMessageId}
                            agentPlanModeSwitching={agentPlanModeSwitching}
                            onContinueAgentPlan={onContinueAgentPlan}
                            onApproveAgentPlan={onApproveAgentPlan}
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
            const groupKey = `merged-${groupMessages.map((message) => message.id).join("-")}`;
            const groupProps: MergedAssistantGroupProps = {
                groupMessages,
                anyShining: groupMessages.some((m) => shiningMessageIds.has(m.id)),
                keepExpanded,
                lastMessageId,
                streamingMessages,
                reasoningExpandStates,
                activitiesByMessageId,
                mcpSliceByMessageId,
                messageInlineInteractionMap,
                shiningMcpCallId,
                allowFeishuDebugResend,
                pendingMessageId,
                actionableAgentPlanMessageId,
                agentPlanModeSwitching,
                onCodeRun,
                onMessageRegenerate,
                onMessageEdit,
                onMessageFork,
                onQueuedMessagePromote,
                onToggleReasoningExpand,
                renderMessageActions,
                resendMessageToFeishuDebug,
                onContinueAgentPlan,
                onApproveAgentPlan,
            };
            const groupEstimatedHeight = estimateMergedGroupHeight(
                groupMessages.map((message) => estimatedHeightByMessageId.get(message.id) ?? 0),
            );

            // 只有历史组会被虚拟化；尾部组留在 last-reply 容器里整体渲染。
            const segmentMessageRuns = keepExpanded
                ? null
                : splitMergedGroupMessages(
                    groupMessages,
                    estimatedHeightByMessageId,
                    groupEstimatedHeight,
                );
            const segments = segmentMessageRuns?.map((segmentMessages, index, runs) => {
                const isFirstSegment = index === 0;
                const isLastSegment = index === runs.length - 1;
                const key = `${groupKey}:segment-${segmentMessages[0].id}`;
                return {
                    key,
                    messageIds: segmentMessages.map((message) => message.id),
                    estimatedHeight: estimateMergedSegmentHeight(
                        segmentMessages.map((message) => estimatedHeightByMessageId.get(message.id) ?? 0),
                        { isFirst: isFirstSegment, isLast: isLastSegment },
                    ),
                    element: (
                        <MergedAssistantGroupSegment
                            key={key}
                            {...groupProps}
                            groupKey={groupKey}
                            anyShining={segmentMessages.some((m) => shiningMessageIds.has(m.id))}
                            segmentMessages={segmentMessages}
                            isFirstSegment={isFirstSegment}
                            isLastSegment={isLastSegment}
                        />
                    ),
                } satisfies MessageElementSegment;
            });

            result.push({
                messageId: lastMsg.id,
                messageIds: groupMessages.map((message) => message.id),
                estimatedHeight: groupEstimatedHeight,
                messageElement: <MergedAssistantGroup key={groupKey} {...groupProps} />,
                segments,
                groupControl,
            });

            currentGroup = [];
        };

        for (const message of allDisplayMessages) {
            if (message.message_type === "user" || message.message_type === "agent_plan") {
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
                            mcpToolCallStates={mcpSliceByMessageId.get(message.id)}
                            shiningMcpCallId={shiningMcpCallId}
                            isLastMessage={isLastMessage}
                            inlineInteractionItems={messageInlineInteractionMap.get(
                                message.id,
                            )}
                            allowFeishuDebugResend={allowFeishuDebugResend}
                            canActOnAgentPlan={message.id === actionableAgentPlanMessageId}
                            agentPlanModeSwitching={agentPlanModeSwitching}
                            onContinueAgentPlan={onContinueAgentPlan}
                            onApproveAgentPlan={onApproveAgentPlan}
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
        mcpSliceByMessageId,
        activitiesByMessageId,
        shiningMcpCallId,
        messageInlineInteractionMap,
        estimatedHeightByMessageId,
        allowFeishuDebugResend,
        lastMessageId,
        isMergeAssistantMessages,
        pendingMessageId,
        resendMessageToFeishuDebug,
        renderMessageActions,
        actionableAgentPlanMessageId,
        agentPlanModeSwitching,
        onContinueAgentPlan,
        onApproveAgentPlan,
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
                || cachedPreviewCodeCount(sourceMessage) > 0;
            const virtualizationMode = shouldKeepLive ? "live" : "virtualized";
            if (entry.segments) {
                entry.segments.forEach((segment, index, segments) => {
                    items.push({
                        key: segment.key,
                        messageId: segment.messageIds[segment.messageIds.length - 1],
                        messageIds: segment.messageIds,
                        estimatedHeight: segment.estimatedHeight,
                        element: segment.element,
                        virtualizationMode,
                        joinsNext: index < segments.length - 1,
                    });
                });
            } else {
                items.push({
                    key: `message-${entry.messageId}`,
                    messageId: entry.messageId,
                    messageIds: entry.messageIds ?? [entry.messageId],
                    estimatedHeight:
                        entry.estimatedHeight
                        ?? estimatedHeightByMessageId.get(entry.messageId)
                        ?? 0,
                    element: entry.messageElement,
                    virtualizationMode,
                });
            }

            const versionElement = versionMap.get(`version-${entry.messageId}`);
            if (versionElement) {
                items.push({
                    key: `version-${entry.messageId}`,
                    messageId: null,
                    estimatedHeight: 48,
                    element: versionElement,
                    virtualizationMode,
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
                        entry.estimatedHeight
                        ?? estimatedHeightByMessageId.get(entry.messageId)
                        ?? 0
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
