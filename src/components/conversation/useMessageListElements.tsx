import { rowIdentity } from "@/utils/composerMessage";
import React, { useContext, useMemo, useRef } from "react";
import { ButlerResultsOnlyContext } from "../butler/ButlerPresentationContext";
import { StreamRetryAttemptSections } from "./StreamRetryAttemptSections";
import { groupMessagesByStreamRetryAttempt, parseStreamRetryMeta, shouldShowStreamRetryGrouping } from "@/utils/streamRetryAttempts";

import MessageItem from "../MessageItem";
import ButlerProgressRow, { ReportBubble } from "@/components/butler/ButlerProgressRow";
import ButlerExchangeRow from "../butler/ButlerExchangeRow";
import VersionPagination from "../VersionPagination";
import { AgentActivityEvent, Message, readScheduledTaskMetadata, StreamEvent } from "../../data/Conversation";
import { isHiddenAgentActivity } from "./AgentActivityList";
import type { InlineInteractionItem } from "../ConversationUI";
import { useDisplayConfig } from "@/hooks/useDisplayConfig";
import { useIsMobilePlatform } from "@/hooks/use-platform";
import { useFeishuDebugResend } from "@/hooks/useFeishuDebugResend";
import MergedAssistantGroup, {
    MergedAssistantGroupSegment,
    type MergedAssistantGroupProps,
} from "./MergedAssistantGroup";
import { splitMergedGroupMessages } from "./mergedGroupSegments";
import { MessageTurnContext } from "./messageTurnContext";
import { LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX } from "./layoutConstants";
import { findLastReplyStartIndex } from "./lastReplyLayout";
import {
    estimateMergedGroupHeight,
    estimateMergedSegmentHeight,
    estimateMessageHeight,
} from "./messageHeightEstimate";

export interface UseMessageListElementsProps {
    isMobile?: boolean;
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
    onQueuedMessageSendNow?: (queueId: number) => void;
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
    /**
     * 覆盖“合并助手消息”显示设置。群聊里连续的非用户消息来自不同成员，
     * 合并成一个气泡会丢掉发言人，所以群聊固定关闭。
     */
    mergeAssistantMessages?: boolean;
    /**
     * 自定义整行：返回元素即替代默认的 MessageItem 行（如群聊的发言人、时间分隔、系统提示），
     * 返回 null 走默认渲染。虚拟化、贴底、跳转高亮、最后一轮布局等列表能力照常生效；
     * 需要参与跳转高亮的行应包含带 data-message-id 的 MessageItem。
     */
    renderMessageRow?: (message: Message) => React.ReactElement | null;
}

export interface MessageElementSegment {
    key: string;
    messageIds: number[];
    estimatedHeight: number;
    element: React.ReactElement;
}

export interface MessageElementEntry {
    messageId: number;
    /** Stable list key; defaults to `message-<messageId>`. */
    rowKey?: string;
    messageIds?: number[];
    estimatedHeight?: number;
    messageElement: React.ReactElement;
    /** Row-sized pieces of a tall merged bubble, used when it is virtualized as history. */
    segments?: MessageElementSegment[];
    groupControl: any;
}

function exchangeEntry(message: Message, isLatestTurn: boolean): MessageElementEntry {
    if (message.message_type === "butler_report") return {
        messageId: message.id, groupControl: null,
        messageElement: <ReportBubble text={message.content} />,
    };
    if (message.butler_interaction) return {
        messageId: message.id, groupControl: null,
        messageElement: (
            <MessageTurnContext.Provider value={isLatestTurn}>
                <div className="w-full">{message.butler_interaction}</div>
            </MessageTurnContext.Provider>
        ),
    };
    if (message.message_type === "butler_processing") {
        return {
            messageId: message.id, groupControl: null, estimatedHeight: 32,
            messageElement: <ButlerProgressRow conversationId={message.conversation_id} publishedReports={message.butler_published_reports} />,
        };
    }
    return { messageId: message.id, messageElement: <ButlerExchangeRow posts={message.butler_posts!} conversationId={message.conversation_id} />, groupControl: null };
}

/** Keep individual bubbles, but put consecutive attempts from one retry session
 * behind the same accessible diagnostics UI as merged mode. Plain history stays
 * as separate virtualized rows; never group across user/plan/exchange boundaries.
 */
export function groupUnmergedRetryEntries(
    messages: Message[],
    entries: MessageElementEntry[],
): MessageElementEntry[] {
    const result: MessageElementEntry[] = [];
    let start = 0;
    while (start < messages.length) {
        const first = messages[start];
        const meta = parseStreamRetryMeta(first);
        const eligible = (message: Message) => message.message_type !== "user"
            && message.message_type !== "agent_plan"
            && message.message_type !== "butler_processing" && !message.butler_posts;
        let end = start + 1;
        if (meta && eligible(first)) {
            // Each retry can allocate a new generation UUID. The persisted retry
            // session, not generation_group_id, identifies the whole attempt chain.
            while (end < messages.length && eligible(messages[end])
                && messages[end].conversation_id === first.conversation_id
                && parseStreamRetryMeta(messages[end])?.sessionId === meta.sessionId) {
                end += 1;
            }
        }
        const run = messages.slice(start, end);
        const rows = entries.slice(start, end);
        if (meta && eligible(first) && shouldShowStreamRetryGrouping(groupMessagesByStreamRetryAttempt(run))) {
            const rowById = new Map(rows.map(row => [row.messageId, row]));
            const last = rows[rows.length - 1];
            result.push({
                messageId: last.messageId,
                messageIds: run.map(message => message.id),
                estimatedHeight: rows.reduce((height, row) => height + (row.estimatedHeight ?? 0), 0),
                groupControl: last.groupControl,
                messageElement: <StreamRetryAttemptSections
                    key={`unmerged-retry-${first.id}`}
                    messages={run}
                    renderMessages={chunk => <>{chunk.map(message => rowById.get(message.id)?.messageElement)}</>}
                />,
            });
        } else {
            result.push(...rows);
        }
        start = end;
    }
    return result;
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
    /**
     * The turn (user message and its replies) this row belongs to. The live
     * footer wraps each turn in a stable element, so a turn that stops being the
     * newest keeps its DOM instead of being reparented and remounted.
     */
    turnKey?: string;
    /** Row belongs to the newest turn, whose wrapper reserves a viewport of space. */
    lastTurn?: boolean;
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
    isMobile = false,
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
    onQueuedMessageSendNow,
    onToggleReasoningExpand,
    inlineInteractionItems,
    allowFeishuDebugResend = false,
    renderMessageActions,
    actionableAgentPlanMessageId = null,
    agentPlanModeSwitching = false,
    onContinueAgentPlan,
    onApproveAgentPlan,
    listWidthPx,
    mergeAssistantMessages,
    renderMessageRow,
}: UseMessageListElementsProps) {
    const displayConfig = useDisplayConfig();
    const isMergeAssistantMessages = mergeAssistantMessages ?? displayConfig.isMergeAssistantMessages;
    const { isShowThinking } = displayConfig;
    const isMobilePlatform = useIsMobilePlatform();
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
            (item) => !item.requireCallMarker && (
                item.messageId === undefined
                || item.messageId === null
                || !displayedMessageIdSet.has(item.messageId)),
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
    // 最近一条真实用户消息之后的内容属于最新一轮；管家时间线里的交互卡片据此决定是否默认收起。
    const latestTurnMessages = useMemo(() => {
        let start = 0;
        for (let index = allDisplayMessages.length - 1; index >= 0; index--) {
            const message = allDisplayMessages[index];
            if (message.message_type === "user" && !message.content?.startsWith("Tool execution results:\n")) {
                start = index + 1;
                break;
            }
        }
        return new Set(allDisplayMessages.slice(start));
    }, [allDisplayMessages]);
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
        const layoutKey = `${isMergeAssistantMessages}:${isShowThinking}:${isMobilePlatform}:${estimateListWidthPx ?? ""}`;
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
                    isMobilePlatform,
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
        isMobilePlatform,
    ]);

    const resultsOnly = useContext(ButlerResultsOnlyContext);
    // A merged bubble that has been the live tail stays whole once it becomes history:
    // re-splitting it into segment rows would remount it right when the next message is
    // sent. Older history is still split, so long transcripts can mount in small pieces.
    const wholeGroupKeysRef = useRef(new Set<string>());

    const messageElements = useMemo(() => {
        const customEntry = (message: Message): MessageElementEntry | null => {
            const element = renderMessageRow?.(message);
            return element ? {
                messageId: message.id,
                messageIds: [message.id],
                estimatedHeight: estimatedHeightByMessageId.get(message.id) ?? 0,
                messageElement: element,
                groupControl: null,
            } : null;
        };

        if (!isMergeAssistantMessages) {
            // 非合并模式：每条消息单独渲染
            const entries = allDisplayMessages.map((message) => {
                if (message.message_type === "butler_report" || message.butler_interaction || message.butler_posts || message.message_type === "butler_processing") return exchangeEntry(message, latestTurnMessages.has(message));
                const custom = customEntry(message);
                if (custom) return custom;
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
                            key={`message-${rowIdentity(message)}`}
                            message={message}
                            streamEvent={streamEvent}
                            onCodeRun={onCodeRun}
                            onMessageRegenerate={() => onMessageRegenerate(message.id)}
                            onMessageEdit={() => onMessageEdit(message)}
                            onMessageFork={() => onMessageFork(message.id)}
                            onQueuedMessagePromote={onQueuedMessagePromote}
                            onQueuedMessageSendNow={onQueuedMessageSendNow}
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
            return resultsOnly ? entries : groupUnmergedRetryEntries(allDisplayMessages, entries);
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
            // Keyed by the first message: replies that join the group while streaming
            // (reasoning -> response, tool rounds) must not remount the bubble.
            const groupKey = `merged-${groupMessages[0].id}`;
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
                onQueuedMessageSendNow,
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
            if (keepExpanded) {
                wholeGroupKeysRef.current.add(groupKey);
            }
            const segmentMessageRuns = keepExpanded || wholeGroupKeysRef.current.has(groupKey)
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
                rowKey: groupKey,
                messageIds: groupMessages.map((message) => message.id),
                estimatedHeight: groupEstimatedHeight,
                messageElement: <MergedAssistantGroup key={groupKey} {...groupProps} />,
                segments,
                groupControl,
            });

            currentGroup = [];
        };

        for (const message of allDisplayMessages) {
            if (message.message_type === "butler_report" || message.butler_interaction || message.butler_posts || message.message_type === "butler_processing") {
                flushGroup({ keepExpanded: false });
                result.push(exchangeEntry(message, latestTurnMessages.has(message)));
                continue;
            }
            const custom = customEntry(message);
            if (custom) {
                // 自定义行不并入相邻气泡，按边界处理
                flushGroup({ keepExpanded: false });
                result.push(custom);
                continue;
            }
            if (message.message_type === "response" && readScheduledTaskMetadata(message.metadata_json)?.role === "result") {
                flushGroup();
                currentGroup.push(message);
                flushGroup({ keepExpanded: message.id === lastMessageId });
                continue;
            }
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
                            key={`message-${rowIdentity(message)}`}
                            message={message}
                            streamEvent={streamEvent}
                            onCodeRun={onCodeRun}
                            onMessageRegenerate={() => onMessageRegenerate(message.id)}
                            onMessageEdit={() => onMessageEdit(message)}
                            onMessageFork={() => onMessageFork(message.id)}
                            onQueuedMessagePromote={onQueuedMessagePromote}
                            onQueuedMessageSendNow={onQueuedMessageSendNow}
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
        latestTurnMessages,
        streamingMessages,
        getGenerationGroupControl,
        shiningMessageIds,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onMessageFork,
        onQueuedMessagePromote,
        onQueuedMessageSendNow,
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
        resultsOnly,
        renderMessageActions,
        actionableAgentPlanMessageId,
        agentPlanModeSwitching,
        onContinueAgentPlan,
        onApproveAgentPlan,
        renderMessageRow,
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
        // Rows before the first user message share a head turn.
        let turnKey = "head";

        const pushMessageWithVersion = (entry: MessageElementEntry, lastTurn: boolean) => {
            const message = messageById.get(entry.messageId);
            const identity = message ? rowIdentity(message) : entry.messageId;
            if (message?.message_type === "user") turnKey = `user-${identity}`;
            const turn = { turnKey, ...(lastTurn ? { lastTurn: true } : {}) };
            const sourceMessage = message ?? allDisplayMessages[0];
            const shouldKeepLive =
                lastTurn
                || sourceMessage?.message_type === "reasoning"
                || cachedPreviewCodeCount(sourceMessage) > 0;
            const virtualizationMode = shouldKeepLive ? "live" : "virtualized";
            if (entry.segments && !lastTurn) {
                entry.segments.forEach((segment, index, segments) => {
                    items.push({
                        key: segment.key,
                        messageId: segment.messageIds[segment.messageIds.length - 1],
                        messageIds: segment.messageIds,
                        estimatedHeight: segment.estimatedHeight,
                        element: segment.element,
                        virtualizationMode,
                        joinsNext: index < segments.length - 1,
                        ...turn,
                    });
                });
            } else {
                items.push({
                    key: entry.rowKey ?? `message-${identity}`,
                    messageId: entry.messageId,
                    messageIds: entry.messageIds ?? [entry.messageId],
                    estimatedHeight:
                        entry.estimatedHeight
                        ?? estimatedHeightByMessageId.get(entry.messageId)
                        ?? 0,
                    element: entry.messageElement,
                    virtualizationMode,
                    ...turn,
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
                    ...turn,
                });
            }
        };

        // The newest turn (from the last user message) stays live; the footer wraps
        // it in the min-height "last reply" container. Its rows keep the same keys
        // they will have as history, so sending the next message does not remount them.
        const hasLastTurn = lastReplyStartIndex >= 0;
        messageElements.forEach((entry, index) => {
            pushMessageWithVersion(entry, hasLastTurn && index >= lastReplyStartIndex);
        });
        const tail = {
            turnKey,
            virtualizationMode: "live" as const,
            ...(hasLastTurn ? { lastTurn: true } : {}),
        };

        placeholderElements.forEach((element, index) => {
            items.push({
                key: `placeholder-${index}`,
                messageId: null,
                estimatedHeight: 96,
                element,
                ...tail,
            });
        });

        if (fallbackInlineInteractionItems.length > 0) {
            items.push({
                key: "inline-interactions",
                messageId: null,
                estimatedHeight: fallbackInlineInteractionItems.length * 112,
                element: (
                    <div className="flex flex-col gap-4 pt-2">
                        {fallbackInlineInteractionItems.map((item) => (
                            <React.Fragment key={item.key}>
                                {item.content}
                            </React.Fragment>
                        ))}
                    </div>
                ),
                ...tail,
            });
        }

        // Desktop input floats over the list; keep room at the end. Mobile input has its own space.
        if (!isMobile && items.length > 0) {
            items.push({
                key: "chat-bottom-spacer",
                messageId: null,
                estimatedHeight: LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX,
                element: (
                    <div
                        className="flex-none"
                        style={{ height: LAST_REPLY_CONTAINER_BOTTOM_SPACER_PX }}
                        data-aipp-slot="chat-bottom-spacer"
                    />
                ),
                ...tail,
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
        isMobile,
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
