import React, { useCallback, useMemo } from "react";
import UnifiedMarkdown from "./UnifiedMarkdown";
import ReasoningMessage from "./ReasoningMessage";
import ErrorMessage from "./message-item/ErrorMessage";
import MessageActionButtons from "./message-item/MessageActionButtons";
import ImageAttachments from "./message-item/ImageAttachments";
import RawTextRenderer from "./RawTextRenderer";
import { ShineBorder } from "./magicui/shine-border";
import { DEFAULT_SHINE_BORDER_CONFIG } from "@/utils/shineConfig";
import { Message, StreamEvent, MCPToolCallUpdateEvent } from "../data/Conversation";
import { useCopyHandler } from "../hooks/useCopyHandler";
import { useCustomTagParser } from "../hooks/useCustomTagParser";
import { useMarkdownConfig } from "../hooks/useMarkdownConfig";
import { useMcpToolCallProcessor } from "../hooks/useMcpToolCallProcessor";
import { useDisplayConfig } from "../hooks/useDisplayConfig";
import { useFeishuDebugResend } from "../hooks/useFeishuDebugResend";
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { maskContent } from "../utils/antiLeakage";
import type { InlineInteractionItem } from "./ConversationUI";
import { ListEnd, Loader2, Zap } from "lucide-react";
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
}

interface QueueMessageMeta {
    queueId: number;
    queueKind: "normal" | "interrupt";
}

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

const RichMessageContent = React.memo(function RichMessageContent({
    displayContent,
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
}: RichMessageContentProps) {
    const { parseCustomTags } = useCustomTagParser();
    const markdownContent = useMemo(
        () => useRawTextRenderer ? displayContent : parseCustomTags(displayContent),
        [displayContent, parseCustomTags, useRawTextRenderer],
    );
    const markdownConfig = useMarkdownConfig({
        onCodeRun,
        disableMarkdownSyntax: isUserMessage && !isUserMessageMarkdownEnabled,
        isStreaming,
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
                    onCodeRun={onCodeRun}
                    isStreaming={isStreaming}
                >
                    {markdownContent}
                </UnifiedMarkdown>
            );

            return processContent(markdownContent, element);
        },
        [
            isUserMessage,
            isUserMessageMarkdownEnabled,
            useRawTextRenderer,
            displayContent,
            markdownContent,
            onCodeRun,
            processContent,
            isStreaming,
        ],
    );
});

function QueuedMessageIndicator({
    meta,
    onPromote,
}: {
    meta: QueueMessageMeta;
    onPromote?: (queueId: number) => void;
}) {
    if (meta.queueKind === "interrupt") {
        return (
            <div
                className="mt-3 flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border bg-background text-muted-foreground"
                title="打断消息"
                aria-label="打断消息"
            >
                <Zap className="h-3.5 w-3.5" />
            </div>
        );
    }

    return (
        <DropdownMenu>
            <DropdownMenuTrigger asChild>
                <button
                    type="button"
                    className="mt-3 flex h-7 w-7 shrink-0 items-center justify-center rounded-md border border-border bg-background text-muted-foreground transition-colors hover:bg-accent hover:text-accent-foreground"
                    title="排队消息"
                    aria-label="排队消息"
                >
                    <ListEnd className="h-3.5 w-3.5" />
                </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
                <DropdownMenuItem onClick={() => onPromote?.(meta.queueId)}>
                    <Zap className="h-4 w-4" />
                    提升为打断消息
                </DropdownMenuItem>
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
    }) => {
        // 防泄露模式
        const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
        const shouldMaskContent = antiLeakageEnabled && !isRevealed && !isLastMessage;
        // 防泄露模式：获取实际显示的内容
        // 流式消息优先使用 streamEvent.content（包含 MCP_TOOL_CALL_STREAMING 标记）
        const displayContent = useMemo(() => {
            const rawContent = (streamEvent && !streamEvent.is_done ? streamEvent.content : null) ?? message.content;
            return shouldMaskContent ? maskContent(rawContent) : rawContent;
        }, [shouldMaskContent, message.content, streamEvent, maskContent]);

        const { copyIconState, handleCopy } = useCopyHandler(displayContent);
        const { isUserMessageMarkdownEnabled, isShowThinking } = useDisplayConfig();
        const { pendingMessageId, resendMessageToFeishuDebug } = useFeishuDebugResend();
        const isFeishuDebugSending = pendingMessageId === message.id;

        const isUserMessage = message.message_type === "user";
        const isStreaming = !!streamEvent && !streamEvent.is_done;
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
                displayContent={displayContent}
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

        // 早期返回：reasoning 类型消息
        if (message.message_type === "reasoning") {
            // 不展示思考过程：思考中时显示加载指示器，思考完成后不渲染
            if (!isShowThinking) {
                const isReasoningComplete = message.finish_time !== null || streamEvent?.is_done === true;
                if (isReasoningComplete) {
                    return null;
                }
                // 思考中：展示一个 loading badge
                return (
                    <div
                        data-message-item
                        data-message-id={message.id}
                        data-message-type="reasoning"
                        className="my-2 flex items-center gap-2 px-3 py-1.5"
                    >
                        <Loader2 className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
                        <span className="text-xs text-muted-foreground">思考中...</span>
                    </div>
                );
            }
            return (
                <ReasoningMessage
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
            return <ErrorMessage content={message.content} messageId={message.id} />;
        }

        // 常规消息渲染
        // 合并模式下不渲染外层气泡包装，只渲染内容
        if (mergedMode && !isUserMessage) {
            return (
                <div data-message-item data-message-id={message.id} data-message-type={message.message_type}>
                    <div className="prose prose-sm max-w-none text-foreground break-all">
                        {richMessageContent}
                    </div>
                    <ImageAttachments
                        attachments={message.attachment_list}
                        conversationId={message.conversation_id}
                        messageId={message.id}
                    />
                </div>
            );
        }

        const bubbleElement = (
            <div
                className="group relative inline-block max-w-[65%] rounded-2xl border border-border bg-background px-5 py-4 text-foreground transition-all duration-200"
            >
                {shouldShowShineBorder && (
                    <ShineBorder
                        shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                        borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                        duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                    />
                )}

                {speakerLabel && (
                    <div className="mb-2 text-xs font-medium text-muted-foreground">
                        {speakerLabel}
                    </div>
                )}

                <div className="prose prose-sm max-w-none text-foreground break-all">
                    {richMessageContent}
                </div>

                <ImageAttachments
                    attachments={message.attachment_list}
                    conversationId={message.conversation_id}
                    messageId={message.id}
                />

                <MessageActionButtons
                    messageId={message.id}
                    messageType={message.message_type}
                    isUserMessage={isUserMessage}
                    copyIconState={copyIconState}
                    onCopy={handleCopy}
                    onEdit={onMessageEdit}
                    onRegenerate={onMessageRegenerate}
                    onFork={onMessageFork}
                    onResendToFeishuDebug={canResendToFeishuDebug ? handleFeishuDebugResend : undefined}
                    isResendToFeishuDebugPending={isFeishuDebugSending}
                    messageContent={message.content}
                />
            </div>
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
                    />
                ) : null}
                {bubbleElement}
            </div>
        );
    }
);

// 自定义比较函数，只在关键属性变化时才重新渲染
const areEqual = (prevProps: MessageItemProps, nextProps: MessageItemProps) => {
    // 基本消息属性比较
    if (prevProps.message.id !== nextProps.message.id) return false;
    if (prevProps.message.content !== nextProps.message.content) return false;
    if (prevProps.message.message_type !== nextProps.message.message_type) return false;
    if (prevProps.message.metadata_json !== nextProps.message.metadata_json) return false;
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

    // 防泄露模式：isLastMessage 变化时需要重新渲染
    if (prevProps.isLastMessage !== nextProps.isLastMessage) return false;

    // 合并模式比较
    if (prevProps.mergedMode !== nextProps.mergedMode) return false;
    if (prevProps.allowFeishuDebugResend !== nextProps.allowFeishuDebugResend) return false;

    return true;
};

MessageItem.displayName = "MessageItem";

export default React.memo(MessageItem, areEqual);
