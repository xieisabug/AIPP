import React, { useCallback, useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import { Message, StreamEvent, MCPToolCallUpdateEvent } from "../data/Conversation";
import { useMcpToolCallProcessor } from "../hooks/useMcpToolCallProcessor";
import { useMarkdownConfig } from "../hooks/useMarkdownConfig";
import { useCustomTagParser } from "../hooks/useCustomTagParser";

interface ReasoningMessageProps {
    message: Message;
    streamEvent?: StreamEvent;
    displayedContent: string;
    isReasoningExpanded: boolean;
    onToggleReasoningExpand?: () => void;
    conversationId?: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    useRawTextRenderer?: boolean; // 脱敏内容使用纯文本渲染，避免 Markdown 解析问题
}

const ReasoningMessage = React.memo(
    ({
        message,
        streamEvent,
        displayedContent,
        isReasoningExpanded,
        onToggleReasoningExpand,
        conversationId,
        mcpToolCallStates,
        useRawTextRenderer = false,
    }: ReasoningMessageProps) => {
        const [currentTime, setCurrentTime] = useState(new Date());

        // 使用 start_time 和 finish_time 来判断思考状态，也考虑 streamEvent 的状态
        const isComplete =
            message.finish_time !== null || streamEvent?.is_done === true;
        const isThinking = message.start_time !== null && !isComplete;

        const { parseCustomTags } = useCustomTagParser();
        const parsedContent = useMemo(
            () => parseCustomTags(displayedContent),
            [displayedContent, parseCustomTags],
        );
        const hasMcpToolCall = useMemo(
            () => /<!--\s*MCP_TOOL_CALL:/.test(parsedContent),
            [parsedContent],
        );

        // Markdown 配置与 MCP 处理（保持与普通消息一致的处理方式）
        const markdownConfig = useMarkdownConfig({ isStreaming: isThinking });
        const { processContent } = useMcpToolCallProcessor(markdownConfig, {
            conversationId,
            messageId: message.id,
            mcpToolCallStates,
        });

        // 为正在思考的消息添加定时器，实时更新显示时间 - 优化更新频率
        useEffect(() => {
            if (!isThinking) {
                return;
            }

            // 根据思考时间调整更新频率
            const getUpdateInterval = () => {
                if (!message.start_time) return 1000;
                const elapsed =
                    Date.now() - new Date(message.start_time).getTime();
                // 思考超过1分钟后，每5秒更新一次
                return elapsed > 60000 ? 5000 : 1000;
            };

            const updateTime = () => setCurrentTime(new Date());

            const timer = setInterval(updateTime, getUpdateInterval());

            return () => clearInterval(timer);
        }, [isThinking, message.start_time]);

        // 计算思考时间 - 统一使用后端时间基准，使用 useMemo 缓存
        const thinkingTime = useMemo(() => {
            // 优先使用 streamEvent 中后端提供的精确时间信息
            if (
                streamEvent?.duration_ms !== undefined &&
                streamEvent.duration_ms > 0
            ) {
                const seconds = Math.floor(streamEvent.duration_ms / 1000);
                if (seconds < 60) return `${seconds}秒`;
                const minutes = Math.floor(seconds / 60);
                const remainingSeconds = seconds % 60;
                return `${minutes}分${remainingSeconds}秒`;
            }

            // 如果有后端提供的结束时间，使用后端时间计算
            if (message.start_time && message.finish_time) {
                const startTime = new Date(message.start_time);
                const endTime = new Date(message.finish_time);
                const diffMs = endTime.getTime() - startTime.getTime();
                const seconds = Math.floor(diffMs / 1000);
                if (seconds < 60) return `${seconds}秒`;
                const minutes = Math.floor(seconds / 60);
                const remainingSeconds = seconds % 60;
                return `${minutes}分${remainingSeconds}秒`;
            }

            // 正在思考时：基于后端开始时间和当前时间计算实时时间
            if (
                message.start_time &&
                !message.finish_time &&
                !streamEvent?.is_done
            ) {
                const startTime = new Date(message.start_time);
                // 使用定时器更新的 currentTime 来保证实时性
                const diffMs = Math.max(
                    0,
                    currentTime.getTime() - startTime.getTime(),
                );
                const seconds = Math.floor(diffMs / 1000);
                if (seconds < 60) return `${seconds}秒`;
                const minutes = Math.floor(seconds / 60);
                const remainingSeconds = seconds % 60;
                return `${minutes}分${remainingSeconds}秒`;
            }

            return "";
        }, [
            streamEvent?.duration_ms,
            streamEvent?.is_done,
            message.start_time,
            message.finish_time,
            currentTime,
        ]);

        // 格式化状态文本
        const formatStatusText = useCallback(
            (baseText: string) => {
                return thinkingTime
                    ? `${baseText}(${baseText === "思考中..." ? "已" : ""}思考 ${thinkingTime})`
                    : baseText;
            },
            [thinkingTime],
        );

        // 缓存内容分割结果
        const contentLines = useMemo(() => {
            const lines = parsedContent.split("\n");
            return {
                lines,
                previewLines: lines.slice(-3), // 思考中时显示最后3行
                hasMoreThanThreeLines: lines.length > 3,
            };
        }, [parsedContent]);

        // 渲染内容（统一使用 useMcpToolCallProcessor，避免重复实现）
        const renderedContent = useMemo(
            () => {
                // 脱敏内容使用纯文本渲染，避免 Markdown 解析问题
                if (useRawTextRenderer) {
                    return <span className="whitespace-pre-wrap break-words">{parsedContent}</span>;
                }
                return processContent(
                    parsedContent,
                    (
                        <ReactMarkdown
                            remarkPlugins={markdownConfig.remarkPlugins as any}
                            rehypePlugins={markdownConfig.rehypePlugins as any}
                            components={markdownConfig.markdownComponents}
                        >
                            {parsedContent}
                        </ReactMarkdown>
                    ),
                );
            },
            [useRawTextRenderer, processContent, parsedContent, markdownConfig.remarkPlugins, markdownConfig.rehypePlugins, markdownConfig.markdownComponents]
        );

        // 渲染预览内容（思考中显示最后 3 行，同样支持 MCP）
        const renderedPreviewContent = useMemo(() => {
            const previewText = contentLines.previewLines.join("\n");
            // 脱敏内容使用纯文本渲染
            if (useRawTextRenderer) {
                return <span className="whitespace-pre-wrap break-words">{previewText}</span>;
            }
            return processContent(
                previewText,
                (
                    <ReactMarkdown
                        remarkPlugins={markdownConfig.remarkPlugins as any}
                        rehypePlugins={markdownConfig.rehypePlugins as any}
                        components={markdownConfig.markdownComponents}
                    >
                        {previewText}
                    </ReactMarkdown>
                ),
            );
        }, [useRawTextRenderer, contentLines.previewLines, processContent, markdownConfig.remarkPlugins, markdownConfig.rehypePlugins, markdownConfig.markdownComponents]);

        // 思考完成时的小模块展示
        if (isComplete && !isReasoningExpanded) {
            return (
                <div
                    data-message-item
                    data-message-id={message.id}
                    data-message-type="reasoning"
                    className="my-2 p-2 bg-gray-50 border-l-4 border-gray-400 rounded-r-lg w-80 max-w-[60%] cursor-pointer hover:bg-gray-100 transition-colors"
                    onClick={() => onToggleReasoningExpand?.()}
                >
                    <div className="flex items-center gap-2">
                        <div className="w-2 h-2 bg-gray-500 rounded-full"></div>
                        <span className="text-sm font-medium text-gray-700">
                            {formatStatusText("思考完成")}
                        </span>
                        <span className="text-xs text-gray-400 ml-auto">
                            点击展开
                        </span>
                    </div>
                    {hasMcpToolCall && (
                        <div className="mt-2 text-sm text-gray-600 whitespace-pre-wrap font-mono">
                            {renderedPreviewContent}
                        </div>
                    )}
                </div>
            );
        }

        // 完整展示（思考完成展开或思考中）
        return (
            <div
                data-message-item
                data-message-id={message.id}
                data-message-type="reasoning"
                className="my-2 p-3 bg-gray-50 border-l-4 border-gray-400 rounded-r-lg max-w-[80%]"
            >
                <div className="flex items-center gap-2 mb-2">
                    <div
                        className={`w-2 h-2 bg-gray-500 rounded-full ${isThinking ? "animate-pulse" : ""}`}
                    ></div>
                    <span className="text-sm font-medium text-gray-700">
                        {formatStatusText(
                            isComplete ? "思考完成" : "思考中...",
                        )}
                    </span>
                </div>
                <div className="text-sm text-gray-600 whitespace-pre-wrap font-mono">
                    {isThinking &&
                        contentLines.hasMoreThanThreeLines &&
                        !isReasoningExpanded ? (
                        <>
                            <div className="text-gray-400 text-xs mb-1">
                                ...
                            </div>
                            {renderedPreviewContent}
                        </>
                    ) : (
                        renderedContent
                    )}
                </div>
                {/* 思考中时的展开按钮 */}
                {isThinking &&
                    contentLines.hasMoreThanThreeLines &&
                    !isReasoningExpanded && (
                        <button
                            onClick={() => onToggleReasoningExpand?.()}
                            className="mt-2 text-xs text-gray-600 hover:text-gray-800 underline cursor-pointer"
                        >
                            展开思考
                        </button>
                    )}
                {/* 思考完成时的收起按钮或思考中展开状态的收起按钮 */}
                {(isComplete || (isThinking && isReasoningExpanded)) && (
                    <button
                        onClick={() => onToggleReasoningExpand?.()}
                        className="mt-2 text-xs text-gray-600 hover:text-gray-800 underline cursor-pointer"
                    >
                        收起
                    </button>
                )}
            </div>
        );
    },
    // 自定义比较函数，只在关键属性变化时才重新渲染
    (prevProps, nextProps) => {
        // 基本消息属性比较
        if (prevProps.message.id !== nextProps.message.id) return false;
        if (prevProps.message.start_time !== nextProps.message.start_time)
            return false;
        if (prevProps.message.finish_time !== nextProps.message.finish_time)
            return false;

        // 显示内容比较
        if (prevProps.displayedContent !== nextProps.displayedContent)
            return false;

        // 展开状态比较
        if (prevProps.isReasoningExpanded !== nextProps.isReasoningExpanded)
            return false;

        // 流式事件比较
        const prevStreamEvent = prevProps.streamEvent;
        const nextStreamEvent = nextProps.streamEvent;
        if (prevStreamEvent?.is_done !== nextStreamEvent?.is_done) return false;
        if (prevStreamEvent?.duration_ms !== nextStreamEvent?.duration_ms)
            return false;

        // 回调函数比较
        if (
            prevProps.onToggleReasoningExpand !==
            nextProps.onToggleReasoningExpand
        )
            return false;

        // MCP 相关属性比较
        if (prevProps.conversationId !== nextProps.conversationId) return false;
        if (prevProps.mcpToolCallStates !== nextProps.mcpToolCallStates) return false;

        // 脱敏模式比较
        if (prevProps.useRawTextRenderer !== nextProps.useRawTextRenderer) return false;

        return true; // 所有关键属性都相同，不需要重新渲染
    },
);

ReasoningMessage.displayName = "ReasoningMessage";

export default ReasoningMessage;
