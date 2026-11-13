import React, { useCallback, useMemo, useState } from "react";
import { ButlerMessageQuoteLabel } from "./butler/ButlerQuote";
import UnifiedMarkdown from "./UnifiedMarkdown";
import { ReasoningDisplay } from "./ReasoningDisplay";
import ErrorMessage from "./message-item/ErrorMessage";
import MessageActionButtons, { type MessageActionButtonsProps } from "./message-item/MessageActionButtons";
import MessageContextMenu from "./message-item/MessageContextMenu";
import ImageAttachments from "./message-item/ImageAttachments";
import RawTextRenderer from "./RawTextRenderer";
import { ShineBorder } from "./magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import { Message, StreamEvent, MCPToolCallUpdateEvent, AgentActivityEvent } from "../data/Conversation";
import { AgentActivityTranscript } from "./conversation/AgentActivityTranscript";
import { AgentPlanCard } from "./conversation/AgentPlanCard";
import { buildActivitySegments } from "./conversation/agent-activity/activitySegments";
import { useCopyHandler } from "../hooks/useCopyHandler";
import { useCustomTagParser } from "../hooks/useCustomTagParser";
import { MarkdownStreamingContext, useMarkdownConfig } from "../hooks/useMarkdownConfig";
import { useMcpToolCallProcessor } from "../hooks/useMcpToolCallProcessor";
import { useDisplayConfig } from "../hooks/useDisplayConfig";
import { useFeishuDebugResend } from "../hooks/useFeishuDebugResend";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { useIsMobilePlatform } from "../hooks/use-platform";
import { maskContent } from "../utils/antiLeakage";
import { recordMessageItemRender } from "../utils/messageItemRenderStats";
import { splitClosedMarkdownBlocks } from "../utils/streamingMarkdownBlocks";
import { resolveMessageDisplayContent } from "../utils/streamingOverlayHandoff";
import { splitTurnContext } from "../utils/turnContext";
import { collapseUserMessage } from "../utils/userMessageCollapse";
import type { InlineInteractionItem } from "./ConversationUI";
import { CalendarClock, ChevronDown, ChevronUp, ListEnd, Send, Zap } from "lucide-react";
import { openScheduleTask } from "../lib/scheduleTaskFocus";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

interface MessageItemProps {
    message: Message;
    streamEvent?: StreamEvent;
    onCodeRun?: (lang: string, code: string) => void;
    onMessageRegenerate?: () => void;
    onMessageEdit?: () => void;
    onMessageFork?: () => void;
    onQueuedMessagePromote?: (queueId: number) => void;
    onQueuedMessageSendNow?: (queueId: number) => void;
    isReasoningExpanded?: boolean;
    onToggleReasoningExpand?: () => void;
    shouldShowShineBorder?: boolean;
    conversationId?: number; // Add conversation_id context
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>; // Add MCP states
    shiningMcpCallId?: number | null;
    isLastMessage?: boolean; // 防泄露模式：是否为最后一条消息
    inlineInteractionItems?: InlineInteractionItem[];
    allowFeishuDebugResend?: boolean;
    mergedMode?: boolean; // 合并模式：不渲染外层气泡包装
    messageActions?: React.ReactNode;
    agentActivities?: AgentActivityEvent[]; // Codex 等 Agent 的活动事件（收进气泡内展示）
    canActOnAgentPlan?: boolean;
    agentPlanModeSwitching?: boolean;
    onContinueAgentPlan?: () => void;
    onApproveAgentPlan?: () => void;
}

interface QueueMessageMeta {
    queueId: number;
    queueKind: "normal" | "interrupt";
}

const USER_MESSAGE_COLLAPSE_THRESHOLD = 300;

interface RichMessageContentProps {
    displayContent: string;
    onCodeRun?: (lang: string, code: string) => void;
    isUserMessage: boolean;
    isUserMessageMarkdownEnabled: boolean;
    isStreaming: boolean;
    isLastMessage: boolean;
    useRawTextRenderer?: boolean;
    conversationId?: number;
    messageId: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    shiningMcpCallId?: number | null;
    inlineInteractionItems?: InlineInteractionItem[];
}

interface StableMarkdownChunkProps extends Omit<RichMessageContentProps, "displayContent"> {
    chunk: string;
}

function stableMarkdownChunkPropsEqual(
    prev: StableMarkdownChunkProps,
    next: StableMarkdownChunkProps,
): boolean {
    return prev.chunk === next.chunk
        && prev.isStreaming === next.isStreaming
        && prev.isUserMessage === next.isUserMessage
        && prev.isUserMessageMarkdownEnabled === next.isUserMessageMarkdownEnabled
        && prev.isLastMessage === next.isLastMessage
        && prev.useRawTextRenderer === next.useRawTextRenderer
        && prev.conversationId === next.conversationId
        && prev.messageId === next.messageId
        && prev.mcpToolCallStates === next.mcpToolCallStates
        && prev.shiningMcpCallId === next.shiningMcpCallId
        && prev.inlineInteractionItems === next.inlineInteractionItems
        && prev.onCodeRun === next.onCodeRun;
}

const StableMarkdownChunk = React.memo(function StableMarkdownChunk({
    chunk: displayContent,
    onCodeRun,
    isUserMessage,
    isUserMessageMarkdownEnabled,
    isStreaming,
    isLastMessage,
    useRawTextRenderer = false,
    conversationId,
    messageId,
    mcpToolCallStates,
    shiningMcpCallId,
    inlineInteractionItems,
}: StableMarkdownChunkProps) {
    const { parseCustomTags } = useCustomTagParser();
    const markdownContent = useMemo(
        () => useRawTextRenderer ? displayContent : parseCustomTags(displayContent),
        [displayContent, parseCustomTags, useRawTextRenderer],
    );
    const markdownConfig = useMarkdownConfig({
        onCodeRun,
        disableMarkdownSyntax: isUserMessage && !isUserMessageMarkdownEnabled,
    });
    const { processContent } = useMcpToolCallProcessor(markdownConfig, {
        conversationId,
        messageId,
        isLastMessage,
        mcpToolCallStates,
        shiningMcpCallId,
        inlineInteractionItems,
    });

    return useMemo(
        () => {
            if (useRawTextRenderer) {
                return <RawTextRenderer content={displayContent} />;
            }

            if (isUserMessage && !isUserMessageMarkdownEnabled) {
                return <RawTextRenderer content={markdownContent} />;
            }

            const element = (
                <UnifiedMarkdown
                    noProseWrapper
                    markdownConfig={markdownConfig}
                    onCodeRun={onCodeRun}
                    isStreaming={isStreaming}
                >
                    {markdownContent}
                </UnifiedMarkdown>
            );

            return (
                <MarkdownStreamingContext.Provider value={isStreaming}>
                    {processContent(markdownContent, element)}
                </MarkdownStreamingContext.Provider>
            );
        },
        [
            isUserMessage,
            isUserMessageMarkdownEnabled,
            useRawTextRenderer,
            displayContent,
            markdownContent,
            markdownConfig,
            onCodeRun,
            processContent,
            isStreaming,
        ],
    );
}, stableMarkdownChunkPropsEqual);

const RichMessageContent = React.memo(function RichMessageContent(props: RichMessageContentProps) {
    // 在本次挂载期间流式输出过的消息，结束后保持同一套切块结构，
    // 否则流结束时整条消息会从切块切回整体渲染，所有块重新挂载。
    const [streamedWhileMounted, setStreamedWhileMounted] = useState(props.isStreaming);
    if (props.isStreaming && !streamedWhileMounted) {
        setStreamedWhileMounted(true);
    }

    const blocks = useMemo(() => {
        if (!streamedWhileMounted || props.useRawTextRenderer || props.isUserMessage) {
            return null;
        }
        if (
            props.displayContent.includes("MCP_TOOL_CALL")
            || props.displayContent.includes("<mcp_tool_call")
        ) {
            return null;
        }
        return splitClosedMarkdownBlocks(props.displayContent);
    }, [
        props.displayContent,
        props.isUserMessage,
        props.useRawTextRenderer,
        streamedWhileMounted,
    ]);

    // 两条路径都以 key="tail" 结尾，切换时尾块不会被重新挂载。
    return (
        <>
            {blocks?.closed.map((chunk, index) => (
                <StableMarkdownChunk
                    key={`closed-${index}`}
                    {...props}
                    chunk={chunk}
                    isStreaming={false}
                />
            ))}
            {!blocks || blocks.open ? (
                <StableMarkdownChunk
                    key="tail"
                    {...props}
                    chunk={blocks ? blocks.open : props.displayContent}
                />
            ) : null}
        </>
    );
});

function QueuedMessageIndicator({
    meta,
    onPromote,
    onSendNow,
}: {
    meta: QueueMessageMeta;
    onPromote?: (queueId: number) => void;
    onSendNow?: (queueId: number) => void;
}) {
    // 移动端触控目标 ≥44px，桌面保持 28px
    const sizeClass = useIsMobilePlatform() ? "h-11 w-11" : "h-7 w-7";
    const isInterrupt = meta.queueKind === "interrupt";
    const label = isInterrupt ? "打断消息" : "排队消息";
    return (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <button
                    type="button"
                    className={`mt-3 flex ${sizeClass} shrink-0 items-center justify-center rounded-md border border-border bg-background text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground`}
                    title={label}
                    aria-label={label}
                >
                    {isInterrupt ? <Zap className="h-3.5 w-3.5" /> : <ListEnd className="h-3.5 w-3.5" />}
                </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
                {onSendNow && (
                    <DropdownMenuItem onClick={() => onSendNow(meta.queueId)}>
                        <Send className="h-4 w-4" />
                        立即发送
                    </DropdownMenuItem>
                )}
                {!isInterrupt && (
                    <DropdownMenuItem onClick={() => onPromote?.(meta.queueId)}>
                        <Zap className="h-4 w-4" />
                        提升为打断消息
                    </DropdownMenuItem>
                )}
            </DropdownMenuContent>
        </DropdownMenu>
    );
}

function areAttachmentListsEqual(prevAttachments?: Array<any>, nextAttachments?: Array<any>) {
    const prevList = prevAttachments ?? [];
    const nextList = nextAttachments ?? [];

    if (prevList.length !== nextList.length) {
        return false;
    }

    for (let index = 0; index < prevList.length; index += 1) {
        const prevAttachment = prevList[index];
        const nextAttachment = nextList[index];

        if (
            prevAttachment?.id !== nextAttachment?.id ||
            prevAttachment?.attachment_type !== nextAttachment?.attachment_type ||
            prevAttachment?.attachment_url !== nextAttachment?.attachment_url ||
            prevAttachment?.attachment_content !== nextAttachment?.attachment_content
        ) {
            return false;
        }
    }

    return true;
}

const MessageItem = React.memo<MessageItemProps>(
    ({
        message,
        streamEvent,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onMessageFork,
        onQueuedMessagePromote,
        onQueuedMessageSendNow,
        isReasoningExpanded = false,
        onToggleReasoningExpand,
        shouldShowShineBorder = false,
        conversationId,
        mcpToolCallStates,
        shiningMcpCallId,
        isLastMessage = false,
        inlineInteractionItems,
        allowFeishuDebugResend = false,
        mergedMode = false,
        messageActions,
        agentActivities,
        canActOnAgentPlan = false,
        agentPlanModeSwitching = false,
        onContinueAgentPlan,
        onApproveAgentPlan,
    }) => {
        recordMessageItemRender(message.id);
        // 防泄露模式
        const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
        const shouldMaskContent = antiLeakageEnabled && !isRevealed && !isLastMessage;
        // 防泄露模式：获取实际显示的内容
        // 流式中用 overlay；is_done 交接时取 overlay 与 message 的较长者，避免尾部字符丢失（LeaderAI #149）
        // 用户消息末尾的本轮附加上下文（时间、记忆、工作记录等）只给模型看：气泡只显示原话。
        const turnContext = useMemo(
            () => message.message_type === "user"
                ? splitTurnContext(resolveMessageDisplayContent(message.content, streamEvent))
                : null,
            [message.message_type, message.content, streamEvent],
        );
        const displayContent = useMemo(() => {
            const rawContent = turnContext ? turnContext.text : resolveMessageDisplayContent(message.content, streamEvent);
            return shouldMaskContent ? maskContent(rawContent) : rawContent;
        }, [turnContext, shouldMaskContent, message.content, streamEvent, maskContent]);

        const { copyIconState, handleCopy } = useCopyHandler(displayContent);
        const { isUserMessageMarkdownEnabled } = useDisplayConfig();
        const { pendingMessageId, resendMessageToFeishuDebug } = useFeishuDebugResend();
        const isFeishuDebugSending = pendingMessageId === message.id;
        const [expandedUserMessage, setExpandedUserMessage] = useState<{
            messageId: number;
            content: string;
        } | null>(null);

        const isUserMessage = message.message_type === "user";
        const isStreaming = !!streamEvent && !streamEvent.is_done;
        const isMobilePlatform = useIsMobilePlatform();
        // 只按显示出来的文字折叠；附件等标签整块保留，不会被截断成原文。
        const collapsedUserContent = useMemo(
            () => isUserMessage ? collapseUserMessage(displayContent, USER_MESSAGE_COLLAPSE_THRESHOLD) : null,
            [displayContent, isUserMessage],
        );
        const shouldCollapseUserMessage = collapsedUserContent !== null;
        const isUserMessageExpanded = expandedUserMessage?.messageId === message.id
            && expandedUserMessage.content === displayContent;
        const renderedDisplayContent = collapsedUserContent !== null && !isUserMessageExpanded
            ? collapsedUserContent
            : displayContent;
        const speakerLabel = useMemo(() => {
            if (!message.metadata_json) {
                return null;
            }
            try {
                const parsed = JSON.parse(message.metadata_json) as Record<string, unknown>;
                const rawLabel = parsed.speakerLabel ?? parsed.speaker_name ?? parsed.speakerName;
                return typeof rawLabel === "string" && rawLabel.trim() ? rawLabel.trim() : null;
            } catch {
                return null;
            }
        }, [message.metadata_json]);

        // 定时任务投递的结果：标出来源，点击可打开该任务
        const scheduledTaskOrigin = useMemo(() => {
            if (!message.metadata_json) return null;
            try {
                const parsed = JSON.parse(message.metadata_json) as Record<string, unknown>;
                if (parsed.origin !== "scheduled_task") return null;
                const taskId = Number(parsed.scheduled_task_id);
                const taskName = typeof parsed.scheduled_task_name === "string" ? parsed.scheduled_task_name : "";
                return Number.isInteger(taskId) && taskId > 0 ? { taskId, taskName } : null;
            } catch {
                return null;
            }
        }, [message.metadata_json]);

        const obsoleteGoal = useMemo(() => {
            if (streamEvent?.end_reason === "goal_superseded") return true;
            try {
                const metadata = JSON.parse(message.metadata_json || "{}");
                return metadata.end_reason === "goal_superseded" || metadata.butler_goal_stale === true;
            } catch { return false; }
        }, [message.metadata_json, streamEvent?.end_reason]);

        const canResendToFeishuDebug =
            allowFeishuDebugResend
            && (message.message_type === "response" || message.message_type === "tool_result");

        const queuedMessageMeta = useMemo<QueueMessageMeta | null>(() => {
            if (!message.metadata_json) {
                return null;
            }
            try {
                const parsed = JSON.parse(message.metadata_json) as Record<string, unknown>;
                if (parsed.queue_status !== "queued") {
                    return null;
                }
                const queueId = Number(parsed.queue_id);
                if (!Number.isFinite(queueId) || queueId <= 0) {
                    return null;
                }
                return {
                    queueId,
                    queueKind: parsed.queue_kind === "interrupt" ? "interrupt" : "normal",
                };
            } catch {
                return null;
            }
        }, [message.metadata_json]);

        const handleFeishuDebugResend = useCallback(async () => {
            if (isFeishuDebugSending || !canResendToFeishuDebug) {
                return;
            }
            await resendMessageToFeishuDebug(message.id);
        }, [canResendToFeishuDebug, isFeishuDebugSending, message.id, resendMessageToFeishuDebug]);

        const richMessageContent = (
            <RichMessageContent
                displayContent={renderedDisplayContent}
                onCodeRun={onCodeRun}
                isUserMessage={isUserMessage}
                isUserMessageMarkdownEnabled={isUserMessageMarkdownEnabled}
                isStreaming={isStreaming}
                isLastMessage={isLastMessage}
                useRawTextRenderer={shouldMaskContent}
                conversationId={conversationId}
                messageId={message.id}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={shiningMcpCallId}
                inlineInteractionItems={inlineInteractionItems}
            />
        );

        // 将活动卡片按 content_offset 穿插到正文对应位置（offset 按 Unicode 字符计）；
        // 无有效偏移的活动（旧数据）退化为统一列在正文之后
        const renderSegmentContent = (text: string, isFinalSegment: boolean) => (
            <div className="prose prose-sm max-w-none text-foreground break-all">
                <RichMessageContent
                    displayContent={text}
                    onCodeRun={onCodeRun}
                    isUserMessage={isUserMessage}
                    isUserMessageMarkdownEnabled={isUserMessageMarkdownEnabled}
                    isStreaming={isStreaming && isFinalSegment}
                    isLastMessage={isLastMessage}
                    useRawTextRenderer={shouldMaskContent}
                    conversationId={conversationId}
                    messageId={message.id}
                    mcpToolCallStates={mcpToolCallStates}
                    shiningMcpCallId={shiningMcpCallId}
                    inlineInteractionItems={inlineInteractionItems}
                />
            </div>
        );

        const activitySegments = agentActivities && agentActivities.length > 0
            ? buildActivitySegments(renderedDisplayContent, agentActivities)
            : null;

        const renderAgentActivities = (activities: AgentActivityEvent[]) => (
            <AgentActivityTranscript
                activities={activities}
                message={message}
                responseComplete={message.finish_time !== null || streamEvent?.is_done === true}
                conversationId={conversationId}
                mcpToolCallStates={mcpToolCallStates}
                shiningMcpCallId={shiningMcpCallId}
                inlineInteractionItems={inlineInteractionItems}
                useRawTextRenderer={shouldMaskContent}
            />
        );

        const rawMessageBodyContent = activitySegments ? (
            <>
                {activitySegments.map((segment, segmentIndex) => (
                    <React.Fragment key={`activity-segment-${segmentIndex}`}>
                        {segment.text.length > 0
                            && renderSegmentContent(segment.text, segmentIndex === activitySegments.length - 1)}
                        {segment.activities.length > 0 && (
                            renderAgentActivities(segment.activities)
                        )}
                    </React.Fragment>
                ))}
            </>
        ) : (
            <>
                <div className="prose prose-sm max-w-none text-foreground break-all">
                    {richMessageContent}
                </div>
                {agentActivities && agentActivities.length > 0 && (
                    renderAgentActivities(agentActivities)
                )}
            </>
        );

        const messageBodyContent = <>
            {obsoleteGoal && <div role="note" className="mb-2 text-xs text-muted-foreground">旧目标的历史回复，不是当前结果</div>}
            {rawMessageBodyContent}
        </>;

        // 定时任务结果前的触发消息：显示成一行分隔提示，不当成用户发言
        if (message.message_type === "user" && scheduledTaskOrigin) {
            return (
                <div className="flex justify-center py-1" data-aipp-slot="message-scheduled-task-trigger">
                    <button
                        type="button"
                        className="inline-flex max-w-full items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                        title="在定时任务里查看"
                        onClick={() => void openScheduleTask(scheduledTaskOrigin.taskId).catch(console.error)}
                    >
                        <CalendarClock className="h-3.5 w-3.5 shrink-0" />
                        <span className="truncate">{message.content.replace(/^\[定时任务\]\s*/, "定时任务 ")}</span>
                    </button>
                </div>
            );
        }

        // 早期返回：reasoning 类型消息
        if (message.message_type === "reasoning") {
            return (
                <ReasoningDisplay
                    message={message}
                    streamEvent={streamEvent}
                    displayedContent={displayContent}
                    isReasoningExpanded={isReasoningExpanded}
                    onToggleReasoningExpand={onToggleReasoningExpand}
                    conversationId={conversationId}
                    mcpToolCallStates={mcpToolCallStates}
                    shiningMcpCallId={shiningMcpCallId}
                    inlineInteractionItems={inlineInteractionItems}
                    useRawTextRenderer={shouldMaskContent}
                />
            );
        }

        // 早期返回：错误类型消息
        if (message.message_type === "error") {
            return <ErrorMessage content={message.content} messageId={message.id} superseded={message.isSupersededError} />;
        }

        if (message.message_type === "agent_plan") {
            return (
                <div
                    className="flex justify-start"
                    data-message-item
                    data-message-id={message.id}
                    data-message-type="agent_plan"
                >
                    <AgentPlanCard
                        content={displayContent}
                        isStreaming={isStreaming}
                        canAct={canActOnAgentPlan && !isStreaming}
                        modeSwitching={agentPlanModeSwitching}
                        onContinuePlanning={onContinueAgentPlan}
                        onApprove={onApproveAgentPlan}
                    />
                </div>
            );
        }

        // 常规消息渲染
        // 合并模式下不渲染外层气泡包装，只渲染内容
        if (mergedMode && !isUserMessage) {
            return (
                <div data-message-item data-message-id={message.id} data-message-type={message.message_type}>
                    <ButlerMessageQuoteLabel message={message} />
                    {messageBodyContent}
                    <ImageAttachments
                        attachments={message.attachment_list}
                        conversationId={message.conversation_id}
                        messageId={message.id}
                    />
                </div>
            );
        }

        const actionProps: MessageActionButtonsProps = {
            messageId: message.id,
            messageType: message.message_type,
            isUserMessage,
            copyIconState,
            onCopy: handleCopy,
            onEdit: onMessageEdit,
            onRegenerate: onMessageRegenerate,
            onFork: onMessageFork,
            onResendToFeishuDebug: canResendToFeishuDebug ? handleFeishuDebugResend : undefined,
            isResendToFeishuDebugPending: isFeishuDebugSending,
            messageContent: turnContext ? turnContext.text : message.content,
            pluginActions: messageActions,
        };

        const bubbleElement = (
            <MessageContextMenu actions={actionProps}>
            <div
                className={`group relative inline-block ${isMobilePlatform ? "aipp-mobile-message-bubble max-w-[92%] min-w-0 px-3 py-2" : "max-w-[65%] px-5 py-4"} rounded-2xl border border-border bg-background text-foreground transition-all duration-200`}
            >
                {shouldShowShineBorder && (
                    <ShineBorder
                        shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                        borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                        duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                    />
                )}

                <ButlerMessageQuoteLabel message={message} />
                {scheduledTaskOrigin && (
                    <button
                        type="button"
                        className="mb-2 inline-flex max-w-full items-center gap-1 rounded text-xs text-muted-foreground hover:text-foreground"
                        title="在定时任务里查看"
                        data-aipp-slot="message-scheduled-task-origin"
                        onClick={() => void openScheduleTask(scheduledTaskOrigin.taskId).catch(console.error)}
                    >
                        <CalendarClock className="h-3.5 w-3.5 shrink-0" />
                        <span className="truncate">来自定时任务《{scheduledTaskOrigin.taskName || `#${scheduledTaskOrigin.taskId}`}》</span>
                    </button>
                )}
                {speakerLabel && (
                    <div className="mb-2 text-xs font-medium text-muted-foreground">
                        {speakerLabel}
                    </div>
                )}

                {messageBodyContent}

                {shouldCollapseUserMessage && (
                    <button
                        type="button"
                        className={`mt-2 ml-auto flex items-center gap-1 text-muted-foreground transition-colors hover:text-foreground ${isMobilePlatform ? "min-h-11 px-3 text-sm" : "text-xs"}`}
                        aria-expanded={isUserMessageExpanded}
                        onClick={() => setExpandedUserMessage(
                            isUserMessageExpanded
                                ? null
                                : { messageId: message.id, content: displayContent },
                        )}
                    >
                        {isUserMessageExpanded ? (
                            <>
                                <ChevronUp className="h-3.5 w-3.5" />
                                收起
                            </>
                        ) : (
                            <>
                                <ChevronDown className="h-3.5 w-3.5" />
                                展开
                            </>
                        )}
                    </button>
                )}

                <ImageAttachments
                    attachments={message.attachment_list}
                    conversationId={message.conversation_id}
                    messageId={message.id}
                />

                <MessageActionButtons {...actionProps} />
            </div>
            </MessageContextMenu>
        );

        return (
            <div
                className={`flex items-start gap-2 ${isUserMessage ? "justify-end" : "justify-start"}`}
                data-message-item
                data-message-id={message.id}
                data-message-type={message.message_type}
            >
                {isUserMessage && queuedMessageMeta ? (
                    <QueuedMessageIndicator
                        meta={queuedMessageMeta}
                        onPromote={onQueuedMessagePromote}
                        onSendNow={onQueuedMessageSendNow}
                    />
                ) : null}
                {bubbleElement}
            </div>
        );
    }
);

// 自定义比较函数，只在关键属性变化时才重新渲染
const areEqual = (prevProps: MessageItemProps, nextProps: MessageItemProps) => {
    if (prevProps.message.isSupersededError !== nextProps.message.isSupersededError) return false;
    // 基本消息属性比较
    if (prevProps.message.id !== nextProps.message.id) return false;
    if (prevProps.message.content !== nextProps.message.content) return false;
    if (prevProps.message.message_type !== nextProps.message.message_type) return false;
    if (prevProps.message.metadata_json !== nextProps.message.metadata_json) return false;
    if (prevProps.message.reply_quote?.label !== nextProps.message.reply_quote?.label
        || prevProps.message.reply_quote?.target !== nextProps.message.reply_quote?.target) return false;
    if (prevProps.message.large_message_preview !== nextProps.message.large_message_preview) {
        return false;
    }
    if (!areAttachmentListsEqual(prevProps.message.attachment_list, nextProps.message.attachment_list)) {
        return false;
    }

    // regenerate 数组比较
    const prevRegenerate = prevProps.message.regenerate;
    const nextRegenerate = nextProps.message.regenerate;
    if (prevRegenerate?.length !== nextRegenerate?.length) return false;

    // 流式事件比较
    const prevStreamEvent = prevProps.streamEvent;
    const nextStreamEvent = nextProps.streamEvent;
    if (prevStreamEvent?.is_done !== nextStreamEvent?.is_done) return false;
    if (prevStreamEvent?.content !== nextStreamEvent?.content) return false;
    if (prevStreamEvent?.end_reason !== nextStreamEvent?.end_reason) return false;

    // reasoning 展开状态比较
    if (prevProps.isReasoningExpanded !== nextProps.isReasoningExpanded) return false;

    // ShineBorder 动画状态比较
    if (prevProps.shouldShowShineBorder !== nextProps.shouldShowShineBorder) return false;

    // Sub-task related props comparison
    if (prevProps.conversationId !== nextProps.conversationId) return false;

    // Re-render when MCP tool call state map updates so tool status can refresh
    if (prevProps.mcpToolCallStates !== nextProps.mcpToolCallStates) return false;
    if (prevProps.shiningMcpCallId !== nextProps.shiningMcpCallId) return false;

    if (prevProps.inlineInteractionItems !== nextProps.inlineInteractionItems) return false;
    if (prevProps.onQueuedMessagePromote !== nextProps.onQueuedMessagePromote) return false;
    if (prevProps.onQueuedMessageSendNow !== nextProps.onQueuedMessageSendNow) return false;

    // 防泄露模式：isLastMessage 变化时需要重新渲染
    if (prevProps.isLastMessage !== nextProps.isLastMessage) return false;

    // 合并模式比较
    if (prevProps.mergedMode !== nextProps.mergedMode) return false;
    if (prevProps.allowFeishuDebugResend !== nextProps.allowFeishuDebugResend) return false;
    if (prevProps.messageActions !== nextProps.messageActions) return false;

    // Agent 活动数组引用比较：无活动的消息传 undefined 保持稳定，
    // 有活动的消息随流式更新重建数组引用，需要重新渲染
    if (prevProps.agentActivities !== nextProps.agentActivities) return false;
    if (prevProps.canActOnAgentPlan !== nextProps.canActOnAgentPlan) return false;
    if (prevProps.agentPlanModeSwitching !== nextProps.agentPlanModeSwitching) return false;
    if (prevProps.onContinueAgentPlan !== nextProps.onContinueAgentPlan) return false;
    if (prevProps.onApproveAgentPlan !== nextProps.onApproveAgentPlan) return false;

    return true;
};

MessageItem.displayName = "MessageItem";

export default React.memo(MessageItem, areEqual);
