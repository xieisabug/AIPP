import React, { useCallback } from 'react';
import { Streamdown } from 'streamdown';
import type { Options } from 'react-markdown';
import McpToolCall from '@/components/McpToolCall';
import { MCPToolCallUpdateEvent } from '@/data/Conversation';

interface McpProcessorOptions {
    remarkPlugins: readonly any[];
    rehypePlugins: readonly any[];
    markdownComponents: NonNullable<Options['components']>;
}

interface ProcessorContext {
    conversationId?: number;
    messageId?: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
}

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

        // 将注释替换为实际的 React 组件
        const parts: React.ReactNode[] = [];
        let lastIndex = 0;

        for (const [index, match] of mcpCalls.entries()) {
            try {
                const data = JSON.parse(match[1]);
                const beforeComment = markdownContent.slice(lastIndex, match.index);

                // 添加注释前的内容
                if (beforeComment.trim()) {
                    parts.push(
                        <Streamdown
                            key={`before-${index}`}
                            remarkPlugins={[...remarkPlugins]}
                            components={markdownComponents}
                        >
                            {beforeComment}
                        </Streamdown>
                    );
                }

                // 添加 MCP 工具调用组件
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
                <Streamdown
                    key="remaining"
                    remarkPlugins={[...remarkPlugins]}
                    components={markdownComponents}
                >
                    {remainingContent}
                </Streamdown>
            );
        }

        return <div>{parts}</div>;
    }, [remarkPlugins, rehypePlugins, markdownComponents, conversationId, messageId, mcpToolCallStates]);

    return { processContent };
};