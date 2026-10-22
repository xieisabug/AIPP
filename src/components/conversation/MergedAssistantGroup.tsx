import React, { useEffect, useRef } from "react";

import type { AgentActivityEvent, Message, StreamEvent } from "../../data/Conversation";
import { ShineBorder } from "../magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import MessageActionButtons from "../message-item/MessageActionButtons";
import MessageItem from "../MessageItem";
import type { InlineInteractionItem } from "../ConversationUI";
import {
    reportMergedGroupSegmentWidth,
    useMergedGroupWidth,
} from "./mergedGroupWidthStore";

export interface MergedAssistantGroupProps {
    groupMessages: Message[];
    anyShining: boolean;
    keepExpanded: boolean;
    lastMessageId: number | null;
    streamingMessages: Map<number, StreamEvent>;
    reasoningExpandStates: Map<number, boolean>;
    activitiesByMessageId: Map<number, AgentActivityEvent[]>;
    mcpSliceByMessageId: Map<number, Map<number, any> | undefined>;
    messageInlineInteractionMap: Map<number, InlineInteractionItem[]>;
    shiningMcpCallId: number | null;
    allowFeishuDebugResend: boolean;
    pendingMessageId: number | null;
    actionableAgentPlanMessageId: number | null;
    agentPlanModeSwitching: boolean;
    onCodeRun: (lang: string, inputStr: string) => void;
    onMessageRegenerate: (messageId: number) => void;
    onMessageEdit: (message: Message) => void;
    onMessageFork: (messageId: number) => void;
    onQueuedMessagePromote?: (queueId: number) => void;
    onToggleReasoningExpand: (messageId: number) => void;
    renderMessageActions?: (message: Message) => React.ReactNode;
    resendMessageToFeishuDebug: (messageId: number) => Promise<void> | void;
    onContinueAgentPlan?: () => void;
    onApproveAgentPlan?: () => void;
}

export interface MergedAssistantGroupSegmentProps extends MergedAssistantGroupProps {
    /** Identifies the whole group; segments of one group share its bubble width. */
    groupKey: string;
    /**
     * Messages rendered in this virtual row; a contiguous run of `groupMessages`.
     * `anyShining` refers to these messages only.
     */
    segmentMessages: Message[];
    isFirstSegment: boolean;
    isLastSegment: boolean;
}

/** px-5 padding plus the 1px side borders of a segment bubble. */
const SEGMENT_HORIZONTAL_CHROME_PX = 42;

function sameMessageList(previous: Message[], next: Message[]): boolean {
    return previous.length === next.length
        && previous.every((message, index) => {
            const other = next[index];
            return message === other
                || (
                    message.id === other.id
                    && message.content === other.content
                    && message.message_type === other.message_type
                    && message.metadata_json === other.metadata_json
                    && message.finish_time === other.finish_time
                    && message.attachment_list === other.attachment_list
                    && message.large_message_preview === other.large_message_preview
                    && message.regenerate?.length === other.regenerate?.length
                );
        });
}

function sharedPropsEqual(
    previous: MergedAssistantGroupProps,
    next: MergedAssistantGroupProps,
    renderedMessages: Message[],
): boolean {
    if (!sameMessageList(previous.groupMessages, next.groupMessages)) return false;
    if (previous.anyShining !== next.anyShining) return false;
    if (previous.keepExpanded !== next.keepExpanded) return false;
    if (previous.lastMessageId !== next.lastMessageId) return false;
    if (previous.shiningMcpCallId !== next.shiningMcpCallId) return false;
    if (previous.allowFeishuDebugResend !== next.allowFeishuDebugResend) return false;
    if (previous.pendingMessageId !== next.pendingMessageId) return false;
    if (previous.actionableAgentPlanMessageId !== next.actionableAgentPlanMessageId) return false;
    if (previous.agentPlanModeSwitching !== next.agentPlanModeSwitching) return false;
    if (previous.onCodeRun !== next.onCodeRun) return false;
    if (previous.onMessageRegenerate !== next.onMessageRegenerate) return false;
    if (previous.onMessageEdit !== next.onMessageEdit) return false;
    if (previous.onMessageFork !== next.onMessageFork) return false;
    if (previous.onQueuedMessagePromote !== next.onQueuedMessagePromote) return false;
    if (previous.onToggleReasoningExpand !== next.onToggleReasoningExpand) return false;
    if (previous.renderMessageActions !== next.renderMessageActions) return false;
    if (previous.resendMessageToFeishuDebug !== next.resendMessageToFeishuDebug) return false;
    if (previous.onContinueAgentPlan !== next.onContinueAgentPlan) return false;
    if (previous.onApproveAgentPlan !== next.onApproveAgentPlan) return false;

    return renderedMessages.every((message) => {
        const previousStream = previous.streamingMessages.get(message.id);
        const nextStream = next.streamingMessages.get(message.id);
        return previous.reasoningExpandStates.get(message.id) === next.reasoningExpandStates.get(message.id)
            && previous.activitiesByMessageId.get(message.id) === next.activitiesByMessageId.get(message.id)
            && previous.mcpSliceByMessageId.get(message.id) === next.mcpSliceByMessageId.get(message.id)
            && previous.messageInlineInteractionMap.get(message.id) === next.messageInlineInteractionMap.get(message.id)
            && previousStream?.content === nextStream?.content
            && previousStream?.is_done === nextStream?.is_done;
    });
}

function mergedAssistantGroupPropsEqual(
    previous: MergedAssistantGroupProps,
    next: MergedAssistantGroupProps,
): boolean {
    return sharedPropsEqual(previous, next, next.groupMessages);
}

function mergedAssistantGroupSegmentPropsEqual(
    previous: MergedAssistantGroupSegmentProps,
    next: MergedAssistantGroupSegmentProps,
): boolean {
    return previous.groupKey === next.groupKey
        && previous.isFirstSegment === next.isFirstSegment
        && previous.isLastSegment === next.isLastSegment
        && sameMessageList(previous.segmentMessages, next.segmentMessages)
        && sharedPropsEqual(previous, next, next.segmentMessages);
}

function MergedGroupParts({
    messages,
    props,
}: {
    messages: Message[];
    props: MergedAssistantGroupProps;
}) {
    const {
        keepExpanded,
        lastMessageId,
        streamingMessages,
        reasoningExpandStates,
        activitiesByMessageId,
        mcpSliceByMessageId,
        messageInlineInteractionMap,
        shiningMcpCallId,
        allowFeishuDebugResend,
        actionableAgentPlanMessageId,
        agentPlanModeSwitching,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onMessageFork,
        onQueuedMessagePromote,
        onToggleReasoningExpand,
        renderMessageActions,
        onContinueAgentPlan,
        onApproveAgentPlan,
    } = props;

    return (
        <div className="flex flex-col gap-2">
            {messages.map((message) => {
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
                        isReasoningExpanded={reasoningExpandStates.get(message.id) || false}
                        onToggleReasoningExpand={() => onToggleReasoningExpand(message.id)}
                        shouldShowShineBorder={false}
                        conversationId={message.conversation_id}
                        mcpToolCallStates={mcpSliceByMessageId.get(message.id)}
                        shiningMcpCallId={shiningMcpCallId}
                        isLastMessage={isLast}
                        inlineInteractionItems={messageInlineInteractionMap.get(message.id)}
                        allowFeishuDebugResend={allowFeishuDebugResend}
                        messageActions={renderMessageActions?.(message)}
                        agentActivities={activitiesByMessageId.get(message.id)}
                        canActOnAgentPlan={message.id === actionableAgentPlanMessageId}
                        agentPlanModeSwitching={agentPlanModeSwitching}
                        onContinueAgentPlan={onContinueAgentPlan}
                        onApproveAgentPlan={onApproveAgentPlan}
                        mergedMode
                    />
                );
            })}
        </div>
    );
}

function MergedGroupToolbar({ props }: { props: MergedAssistantGroupProps }) {
    const {
        groupMessages,
        allowFeishuDebugResend,
        pendingMessageId,
        onMessageRegenerate,
        onMessageFork,
        renderMessageActions,
        resendMessageToFeishuDebug,
    } = props;
    const lastResponse = [...groupMessages].reverse().find((message) => message.message_type === "response");
    const resendTargetMessage = [...groupMessages]
        .reverse()
        .find((message) => message.message_type === "response" || message.message_type === "tool_result");
    const toolbarMessage = lastResponse ?? resendTargetMessage;
    if (!toolbarMessage) {
        return null;
    }

    return (
        <MessageActionButtons
            messageId={toolbarMessage.id}
            messageType={lastResponse ? "response" : toolbarMessage.message_type}
            isUserMessage={false}
            copyIconState="copy"
            onCopy={() => {
                const responseContent = groupMessages
                    .filter((message) => message.message_type === "response")
                    .map((message) => message.content);
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
            pluginActions={renderMessageActions?.(toolbarMessage)}
        />
    );
}

function MergedAssistantGroup(props: MergedAssistantGroupProps) {
    const { groupMessages, anyShining } = props;
    const lastMsg = groupMessages[groupMessages.length - 1];

    return (
        <div
            className="flex flex-col"
            data-message-item
            data-message-id={lastMsg.id}
            data-message-type="merged"
        >
            <div className="group relative inline-block max-w-[65%] self-start rounded-2xl border border-border bg-background px-5 py-4 text-foreground transition-all duration-200">
                {anyShining && (
                    <ShineBorder
                        shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                        borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                        duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                    />
                )}
                <MergedGroupParts messages={groupMessages} props={props} />
                <MergedGroupToolbar props={props} />
            </div>
        </div>
    );
}

/**
 * One virtual row of a tall merged bubble. Segments of a group stack without a
 * row gap and draw the bubble border piecewise, so a group reads as a single
 * bubble while only the visible segments are mounted.
 */
function MergedAssistantGroupSegmentComponent(props: MergedAssistantGroupSegmentProps) {
    const { groupKey, segmentMessages, isFirstSegment, isLastSegment, anyShining } = props;
    const lastMsg = segmentMessages[segmentMessages.length - 1];
    const contentRef = useRef<HTMLDivElement | null>(null);
    const groupWidth = useMergedGroupWidth(groupKey);

    useEffect(() => {
        const content = contentRef.current;
        if (!content) {
            return;
        }
        // 内容层是 w-fit，宽度不受组宽影响；加上气泡左右 padding/border 即本段自然宽度。
        const observer = new ResizeObserver((entries) => {
            const entry = entries[0];
            const width = entry?.borderBoxSize?.[0]?.inlineSize ?? entry?.contentRect.width ?? 0;
            if (width > 0) {
                reportMergedGroupSegmentWidth(groupKey, width + SEGMENT_HORIZONTAL_CHROME_PX);
            }
        });
        observer.observe(content);
        return () => observer.disconnect();
    }, [groupKey]);

    const chromeClassName = [
        isFirstSegment ? "rounded-t-2xl border-t pt-4" : "",
        isLastSegment ? "group rounded-b-2xl border-b pb-4" : "pb-2",
    ].join(" ");

    return (
        <div
            className="flex flex-col"
            data-message-item
            data-message-id={lastMsg.id}
            data-message-type="merged"
            data-merged-segment={isFirstSegment ? "first" : isLastSegment ? "last" : "middle"}
        >
            <div
                className={`relative max-w-[65%] self-start border-x border-border bg-background px-5 text-foreground ${chromeClassName}`}
                style={groupWidth > 0 ? { minWidth: `min(${groupWidth}px, 65%)` } : undefined}
            >
                {anyShining && (
                    <ShineBorder
                        shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                        borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                        duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                    />
                )}
                <div ref={contentRef} className="w-fit max-w-full">
                    <MergedGroupParts messages={segmentMessages} props={props} />
                </div>
                {isLastSegment && <MergedGroupToolbar props={props} />}
            </div>
        </div>
    );
}

export const MergedAssistantGroupSegment = React.memo(
    MergedAssistantGroupSegmentComponent,
    mergedAssistantGroupSegmentPropsEqual,
);

export default React.memo(MergedAssistantGroup, mergedAssistantGroupPropsEqual);
