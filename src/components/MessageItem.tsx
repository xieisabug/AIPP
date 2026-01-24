import React, { useMemo, useState } from "react";
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
import { useAntiLeakage } from "../contexts/AntiLeakageContext";
import { maskContent } from "../utils/antiLeakage";
import { SubTaskList, SubTaskDetailDialog } from "./sub-task";
import { SubTaskExecutionSummary } from "../data/SubTask";

interface MessageItemProps {
    message: Message;
    streamEvent?: StreamEvent;
    onCodeRun?: (lang: string, code: string) => void;
    onMessageRegenerate?: () => void;
    onMessageEdit?: () => void;
    onMessageFork?: () => void;
    isReasoningExpanded?: boolean;
    onToggleReasoningExpand?: () => void;
    shouldShowShineBorder?: boolean;
    conversationId?: number; // Add conversation_id context
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>; // Add MCP states
    isLastMessage?: boolean; // 防泄露模式：是否为最后一条消息
}

const MessageItem = React.memo<MessageItemProps>(
    ({
        message,
        streamEvent,
        onCodeRun,
        onMessageRegenerate,
        onMessageEdit,
        onMessageFork,
        isReasoningExpanded = false,
        onToggleReasoningExpand,
        shouldShowShineBorder = false,
        conversationId,
        mcpToolCallStates,
        isLastMessage = false,
    }) => {
        // 防泄露模式
        const { enabled: antiLeakageEnabled, isRevealed } = useAntiLeakage();
        const shouldMaskContent = antiLeakageEnabled && !isRevealed && !isLastMessage;

        // Sub-task detail dialog state
        const [selectedSubTask, setSelectedSubTask] = useState<SubTaskExecutionSummary | null>(null);
        const [isDetailDialogOpen, setIsDetailDialogOpen] = useState(false);

        // Handle sub-task detail view
        const handleSubTaskDetailView = (execution: SubTaskExecutionSummary) => {
            setSelectedSubTask(execution);
            setIsDetailDialogOpen(true);
        };

        const handleCloseDetailDialog = () => {
            setIsDetailDialogOpen(false);
            setSelectedSubTask(null);
        };

        // 防泄露模式：获取实际显示的内容
        const displayContent = useMemo(() => {
            return shouldMaskContent ? maskContent(message.content) : message.content;
        }, [shouldMaskContent, message.content, maskContent]);

        const { copyIconState, handleCopy } = useCopyHandler(displayContent);
        const { parseCustomTags } = useCustomTagParser();
        const { isUserMessageMarkdownEnabled } = useDisplayConfig();

        // 统一的 Markdown 配置，根据用户消息类型和配置决定是否禁用 Markdown 语法
        const isUserMessage = message.message_type === "user";
        const isStreaming = !!streamEvent && !streamEvent.is_done;
        const markdownConfig = useMarkdownConfig({
            onCodeRun,
            disableMarkdownSyntax: isUserMessage && !isUserMessageMarkdownEnabled,
            isStreaming,
        });

        const { processContent } = useMcpToolCallProcessor(markdownConfig, {
            conversationId,
            messageId: message.id,
            mcpToolCallStates,
        });

        // 处理自定义标签解析
        const markdownContent = useMemo(
            () => parseCustomTags(displayContent),
            [displayContent, parseCustomTags]
        );

        const computedTtftMs = useMemo(() => {
            if (message.ttft_ms !== null && message.ttft_ms !== undefined) {
                return message.ttft_ms;
            }

            if (streamEvent?.ttft_ms !== null && streamEvent?.ttft_ms !== undefined) {
                return streamEvent.ttft_ms;
            }

            const startTime = message.start_time ? new Date(message.start_time) : null;
            const firstTokenTime = message.first_token_time ? new Date(message.first_token_time) : null;

            if (startTime && firstTokenTime) {
                const diff = firstTokenTime.getTime() - startTime.getTime();
                return diff > 0 ? diff : null;
            }

            return null;
        }, [
            message.first_token_time,
            message.start_time,
            message.ttft_ms,
            streamEvent?.ttft_ms,
        ]);

        const computedTps = useMemo(() => {
            if (message.tps !== null && message.tps !== undefined) {
                return message.tps;
            }

            if (streamEvent?.tps !== null && streamEvent?.tps !== undefined) {
                return streamEvent.tps;
            }

            const tokenCandidates = [
                message.output_token_count,
                message.token_count,
                message.input_token_count + message.output_token_count,
                streamEvent?.output_token_count,
                streamEvent?.token_count,
            ].filter((value): value is number => typeof value === "number" && value > 0);

            const tokensForSpeed = tokenCandidates.length > 0 ? tokenCandidates[0] : 0;
            if (tokensForSpeed <= 0) {
                return 0;
            }

            const startFallback = message.start_time
                ? new Date(message.start_time)
                : message.created_time
                    ? new Date(message.created_time)
                    : null;

            let startPoint = message.first_token_time
                ? new Date(message.first_token_time)
                : startFallback;

            let finishTime = message.finish_time
                ? new Date(message.finish_time)
                : streamEvent?.end_time
                    ? new Date(streamEvent.end_time)
                    : startPoint && streamEvent?.duration_ms && streamEvent.duration_ms > 0
                        ? new Date(startPoint.getTime() + streamEvent.duration_ms)
                        : null;

            // Backward-compat: finish_time may have second precision while first_token_time has ms.
            if (
                finishTime &&
                startPoint &&
                !Number.isNaN(finishTime.getTime()) &&
                !Number.isNaN(startPoint.getTime()) &&
                finishTime.getMilliseconds() === 0 &&
                Math.floor(finishTime.getTime() / 1000) === Math.floor(startPoint.getTime() / 1000) &&
                startPoint.getMilliseconds() > 0
            ) {
                finishTime = new Date(finishTime.getTime() + 999);
            }

            const effectiveFinish = finishTime ?? (startPoint ? new Date() : null);

            if (!startPoint || !effectiveFinish || Number.isNaN(startPoint.getTime()) || Number.isNaN(effectiveFinish.getTime())) {
                return 0;
            }

            // If finish_time has lower precision (e.g. seconds) it can be <= first_token_time (ms).
            // Fall back to start_time/created_time to avoid negative/zero durations.
            if (effectiveFinish.getTime() <= startPoint.getTime() && startFallback && !Number.isNaN(startFallback.getTime())) {
                startPoint = startFallback;
            }

            let durationMs = Math.max(1, effectiveFinish.getTime() - startPoint.getTime());

            // Backward-compat: older non-stream records stored start/finish too close (or with low precision)
            // but kept total request duration in ttft_ms. Prefer it when it's clearly larger.
            if (typeof message.ttft_ms === "number" && Number.isFinite(message.ttft_ms) && message.ttft_ms > durationMs) {
                durationMs = Math.max(1, message.ttft_ms);
            }

            return (tokensForSpeed * 1000) / durationMs;
        }, [
            message.first_token_time,
            message.finish_time,
            message.output_token_count,
            message.start_time,
            message.created_time,
            message.token_count,
            message.input_token_count,
            message.ttft_ms,
            message.tps,
            streamEvent?.duration_ms,
            streamEvent?.end_time,
            streamEvent?.output_token_count,
            streamEvent?.token_count,
            streamEvent?.tps,
        ]);

        // 渲染内容 - 根据用户消息类型和配置选择渲染方式
        const contentElement = useMemo(
            () => {
                // 如果内容被脱敏（防泄露模式），使用 RawTextRenderer 避免 Markdown 解析星号
                if (shouldMaskContent) {
                    return <RawTextRenderer content={displayContent} />;
                }

                // 如果是用户消息且禁用了 Markdown 渲染，使用 RawTextRenderer
                if (isUserMessage && !isUserMessageMarkdownEnabled) {
                    return <RawTextRenderer content={markdownContent} />;
                }

                // 否则使用统一的 UnifiedMarkdown 渲染
                const element = (
                    <UnifiedMarkdown
                        // 使用 noProseWrapper，避免嵌套重复 prose 容器
                        noProseWrapper
                        onCodeRun={onCodeRun}
                        isStreaming={isStreaming}
                    >
                        {markdownContent}
                    </UnifiedMarkdown>
                );

                // MCP 工具调用后处理
                return processContent(markdownContent, element);
            },
            [shouldMaskContent, displayContent, markdownContent, onCodeRun, processContent, isUserMessage, isUserMessageMarkdownEnabled, isStreaming]
        );

        // 早期返回：reasoning 类型消息
        if (message.message_type === "reasoning") {
            return (
                <ReasoningMessage
                    message={message}
                    streamEvent={streamEvent}
                    displayedContent={displayContent}
                    isReasoningExpanded={isReasoningExpanded}
                    onToggleReasoningExpand={onToggleReasoningExpand}
                    conversationId={conversationId}
                    mcpToolCallStates={mcpToolCallStates}
                    useRawTextRenderer={shouldMaskContent}
                />
            );
        }

        // 早期返回：错误类型消息
        if (message.message_type === "error") {
            return <ErrorMessage content={message.content} messageId={message.id} />;
        }

        // 常规消息渲染
        return (
            <div className="flex flex-col" data-message-item data-message-id={message.id} data-message-type={message.message_type}>
                {/* Message-level sub-tasks - shown at the top of each message */}
                {conversationId && (
                    <SubTaskList
                        conversation_id={conversationId}
                        message_id={message.id}
                        autoLoad={false}
                        onTaskDetailView={handleSubTaskDetailView}
                        className="mb-2"
                    />
                )}

                <div
                    className={`group relative py-4 px-5 rounded-2xl inline-block max-w-[65%] transition-all duration-200 bg-background text-foreground border border-border ${isUserMessage ? "self-end" : "self-start"
                        }`}
                >
                    {shouldShowShineBorder && (
                        <ShineBorder
                            shineColor={DEFAULT_SHINE_BORDER_CONFIG.shineColor}
                            borderWidth={DEFAULT_SHINE_BORDER_CONFIG.borderWidth}
                            duration={DEFAULT_SHINE_BORDER_CONFIG.duration}
                        />
                    )}

                    <div className="prose prose-sm max-w-none text-foreground">
                        {/* RawTextRenderer 已包含 prose 样式，条件渲染避免重复包装 */}
                        {isUserMessage && !isUserMessageMarkdownEnabled ? contentElement : <div>{contentElement}</div>}
                    </div>

                    <ImageAttachments attachments={message.attachment_list} />

                    <MessageActionButtons
                        messageType={message.message_type}
                        isUserMessage={isUserMessage}
                        copyIconState={copyIconState}
                        onCopy={handleCopy}
                        onEdit={onMessageEdit}
                        onRegenerate={onMessageRegenerate}
                        onFork={onMessageFork}
                        tokenCount={message.token_count}
                        inputTokenCount={message.input_token_count}
                        outputTokenCount={message.output_token_count}
                        ttftMs={computedTtftMs}
                        tps={computedTps}
                    />
                </div>

                {/* Sub-task detail dialog */}
                {selectedSubTask && (
                    <SubTaskDetailDialog
                        isOpen={isDetailDialogOpen}
                        onClose={handleCloseDetailDialog}
                        execution={selectedSubTask}
                    // 不再需要传递source_id，使用UI专用的详情接口
                    />
                )}
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

    // 防泄露模式：isLastMessage 变化时需要重新渲染
    if (prevProps.isLastMessage !== nextProps.isLastMessage) return false;

    return true;
};

MessageItem.displayName = "MessageItem";

export default React.memo(MessageItem, areEqual);
