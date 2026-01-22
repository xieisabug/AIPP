import React, { useCallback, useState } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import McpToolCall from '@/components/McpToolCall';
import { MCPToolCallUpdateEvent } from '@/data/Conversation';
import { customUrlTransform } from '@/constants/markdown';
import { Button } from '@/components/ui/button';
import { Send, Loader2 } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';

interface McpProcessorOptions {
    remarkPlugins: readonly any[];
    rehypePlugins: readonly any[];
    markdownComponents: Components;
}

interface ProcessorContext {
    conversationId?: number;
    messageId?: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
}

interface ToolCallData {
    server_name?: string;
    tool_name?: string;
    parameters?: string;
    call_id?: number;
}

const McpToolCallResultsButton: React.FC<{
    toolCallIds: number[];
    mcpToolCallStates: Map<number, MCPToolCallUpdateEvent> | undefined;
    messageId: number | undefined;
}> = ({ toolCallIds, mcpToolCallStates, messageId }) => {
    const [isSending, setIsSending] = useState(false);

    // 检查是否所有工具调用都已完成（非 pending/executing）
    const allCompleted = React.useMemo(() => {
        if (!mcpToolCallStates || toolCallIds.length === 0) {
            return false;
        }
        // 只有当工具调用数量 >= 2 时才显示按钮
        if (toolCallIds.length < 2) {
            return false;
        }
        return toolCallIds.every((id) => {
            const state = mcpToolCallStates.get(id);
            return state && (state.status === 'success' || state.status === 'failed');
        });
    }, [mcpToolCallStates, toolCallIds]);

    const handleSendResults = useCallback(async () => {
        if (!messageId || isSending) {
            return;
        }
        setIsSending(true);
        try {
            await invoke('send_mcp_tool_results', { messageId });
        } catch (error) {
            console.error('Failed to send tool results:', error);
            const errorMessage = error instanceof Error ? error.message : '发送结果失败';
            console.error(errorMessage);
        } finally {
            setIsSending(false);
        }
    }, [messageId, isSending]);

    if (!allCompleted) {
        return null;
    }

    // 统计成功和失败数量
    const successCount = toolCallIds.filter((id) => {
        const state = mcpToolCallStates?.get(id);
        return state?.status === 'success';
    }).length;
    const failedCount = toolCallIds.length - successCount;

    return (
        <div className="w-full max-w-[600px] mt-2 mb-2 flex justify-center">
            <Button
                onClick={handleSendResults}
                disabled={isSending}
                variant="default"
                className="flex items-center gap-2"
            >
                {isSending ? (
                    <>
                        <Loader2 className="h-4 w-4 animate-spin" />
                        发送中...
                    </>
                ) : (
                    <>
                        <Send className="h-4 w-4" />
                        发送结果
                    </>
                )}
                <span className="text-xs opacity-70">
                    ({successCount} 成功, {failedCount} 失败)
                </span>
            </Button>
        </div>
    );
};

export const useMcpToolCallProcessor = (options: McpProcessorOptions, context?: ProcessorContext) => {
    const { remarkPlugins, rehypePlugins, markdownComponents } = options;
    const { conversationId, messageId, mcpToolCallStates } = context || {};

    const processContent = useCallback((
        markdownContent: string,
        fallbackElement: React.ReactElement
    ): React.ReactElement => {
        // 检查是否包含 MCP_TOOL_CALL 注释
        const mcpMatches = markdownContent.matchAll(/<!-- MCP_TOOL_CALL:(.*?) -->/g);
        const mcpCalls = Array.from(mcpMatches);

        if (mcpCalls.length === 0) {
            return fallbackElement;
        }

        console.log(
            "[MCP] detected MCP_TOOL_CALL comments",
            mcpCalls.map((match) => match[1]),
            { conversationId, messageId },
        );

        // 收集所有工具调用数据
        const toolCallDataList: ToolCallData[] = [];
        const toolCallIds: number[] = [];

        // 将注释替换为实际的 React 组件
        const parts: React.ReactNode[] = [];
        let lastIndex = 0;

        for (const [index, match] of mcpCalls.entries()) {
            try {
                const data = JSON.parse(match[1]) as ToolCallData;
                toolCallDataList.push(data);
                if (data.call_id) {
                    toolCallIds.push(data.call_id);
                }

                const beforeComment = markdownContent.slice(lastIndex, match.index);

                // 添加注释前的内容
                if (beforeComment.trim()) {
                    parts.push(
                        <ReactMarkdown
                            key={`before-${index}`}
                            children={beforeComment}
                            remarkPlugins={[...remarkPlugins]}
                            rehypePlugins={[...rehypePlugins]}
                            components={markdownComponents}
                            urlTransform={customUrlTransform}
                        />
                    );
                }

                // 添加 MCP 工具调用组件
                // 只有最后一个工具调用在执行成功后才触发续写
                const isLastCall = index === mcpCalls.length - 1;
                parts.push(
                    <McpToolCall
                        key={`mcp-${index}`}
                        serverName={data.server_name}
                        toolName={data.tool_name}
                        parameters={data.parameters}
                        conversationId={conversationId}
                        messageId={messageId}
                        callId={data.call_id} // 传递 callId，如果存在的话
                        mcpToolCallStates={mcpToolCallStates} // 传递全局 MCP 状态
                        isLastCall={isLastCall} // 是否是最后一个工具调用
                    />
                );

                lastIndex = match.index! + match[0].length;
            } catch (error) {
                console.error('Error parsing MCP_TOOL_CALL data:', error);
            }
        }

        // 添加剩余的内容
        const remainingContent = markdownContent.slice(lastIndex);
        if (remainingContent.trim()) {
            parts.push(
                <ReactMarkdown
                    key="remaining"
                    children={remainingContent}
                    remarkPlugins={[...remarkPlugins]}
                    rehypePlugins={[...rehypePlugins]}
                    components={markdownComponents}
                    urlTransform={customUrlTransform}
                />
            );
        }

        // 添加"发送结果"按钮（如果有多工具调用且都已完成）
        if (toolCallIds.length >= 2) {
            parts.push(
                <McpToolCallResultsButton
                    key="send-results-button"
                    toolCallIds={toolCallIds}
                    mcpToolCallStates={mcpToolCallStates}
                    messageId={messageId}
                />
            );
        }

        return <div>{parts}</div>;
    }, [remarkPlugins, rehypePlugins, markdownComponents, conversationId, messageId, mcpToolCallStates]);

    return { processContent };
};
