import React, { useCallback, useState } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import McpToolCall from '@/components/McpToolCall';
import { MCPToolCallUpdateEvent } from '@/data/Conversation';
import { customUrlTransform } from '@/constants/markdown';
import { Button } from '@/components/ui/button';
import { Send, Loader2 } from 'lucide-react';
import { invoke } from '@tauri-apps/api/core';
import type { InlineInteractionItem } from '@/components/ConversationUI';
import { getErrorMessage } from '@/utils/error';

interface McpProcessorOptions {
    remarkPlugins: readonly any[];
    rehypePlugins: readonly any[];
    markdownComponents: Components;
}

interface ProcessorContext {
    conversationId?: number;
    messageId?: number;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    shiningMcpCallId?: number | null;
    inlineInteractionItems?: InlineInteractionItem[];
}

interface ToolCallData {
    server_name?: string;
    tool_name?: string;
    parameters?: string;
    call_id?: number;
}

interface ParsedMcpToolCallComment {
    start: number;
    end: number;
    complete: boolean;
    data: ToolCallData;
}

const MCP_TOOL_CALL_KEY = "MCP_TOOL_CALL";

function decodeJsonString(raw: string): string {
    try {
        return JSON.parse(`"${raw}"`);
    } catch {
        return raw;
    }
}

function normalizeToolCallData(raw: unknown): ToolCallData {
    if (typeof raw === "number" && Number.isFinite(raw)) {
        return { call_id: raw };
    }
    if (typeof raw === "string" && /^\d+$/.test(raw.trim())) {
        return { call_id: Number.parseInt(raw.trim(), 10) };
    }
    if (!raw || typeof raw !== "object") {
        return {};
    }
    const value = raw as Record<string, unknown>;
    const callIdRaw = value.call_id;
    const callId =
        typeof callIdRaw === "number"
            ? callIdRaw
            : typeof callIdRaw === "string" && /^\d+$/.test(callIdRaw.trim())
                ? Number.parseInt(callIdRaw.trim(), 10)
                : undefined;
    return {
        server_name: typeof value.server_name === "string" ? value.server_name : undefined,
        tool_name: typeof value.tool_name === "string" ? value.tool_name : undefined,
        parameters: typeof value.parameters === "string" ? value.parameters : undefined,
        call_id: callId,
    };
}

function parseToolCallPayload(rawPayload: string): ToolCallData | null {
    const payload = rawPayload.trim();
    if (!payload) return null;
    try {
        return normalizeToolCallData(JSON.parse(payload));
    } catch {
        return null;
    }
}

function parsePartialToolCallPayload(rawPayload: string): ToolCallData {
    const result: ToolCallData = {};
    const serverMatch = rawPayload.match(/"server_name"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (serverMatch) {
        result.server_name = decodeJsonString(serverMatch[1]);
    }
    const toolMatch = rawPayload.match(/"tool_name"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (toolMatch) {
        result.tool_name = decodeJsonString(toolMatch[1]);
    }
    const callIdMatch = rawPayload.match(/"call_id"\s*:\s*(\d+)/);
    if (callIdMatch) {
        result.call_id = Number.parseInt(callIdMatch[1], 10);
    }
    return result;
}

function parsePartialXmlToolCallPayload(rawPayload: string): ToolCallData {
    const result: ToolCallData = {};
    const serverMatch = rawPayload.match(/<server_name>([\s\S]*?)(?:<\/server_name>|$)/i);
    if (serverMatch) {
        result.server_name = serverMatch[1].trim();
    }
    const toolMatch = rawPayload.match(/<tool_name>([\s\S]*?)(?:<\/tool_name>|$)/i);
    if (toolMatch) {
        result.tool_name = toolMatch[1].trim();
    }
    const paramsMatch = rawPayload.match(/<parameters>([\s\S]*?)(?:<\/parameters>|$)/i);
    if (paramsMatch) {
        result.parameters = paramsMatch[1].trim();
    }
    return result;
}

function findJsonObjectEnd(content: string, startIndex: number): number | null {
    if (startIndex >= content.length || content[startIndex] !== "{") {
        return null;
    }
    let depth = 0;
    let inString = false;
    let escaped = false;
    for (let i = startIndex; i < content.length; i++) {
        const char = content[i];
        if (inString) {
            if (escaped) {
                escaped = false;
                continue;
            }
            if (char === "\\") {
                escaped = true;
                continue;
            }
            if (char === '"') {
                inString = false;
            }
            continue;
        }
        if (char === '"') {
            inString = true;
            continue;
        }
        if (char === "{") {
            depth += 1;
            continue;
        }
        if (char === "}") {
            depth -= 1;
            if (depth === 0) {
                return i;
            }
        }
    }
    return null;
}

function findNextMcpToolCallStart(
    content: string,
    fromIndex: number,
): { commentStart: number; payloadStart: number } | null {
    let cursor = fromIndex;
    while (cursor < content.length) {
        const commentStart = content.indexOf("<!--", cursor);
        if (commentStart === -1) {
            return null;
        }

        let head = commentStart + 4;
        while (head < content.length && /\s/.test(content[head])) {
            head += 1;
        }

        if (!content.startsWith(MCP_TOOL_CALL_KEY, head)) {
            cursor = commentStart + 4;
            continue;
        }

        let payloadStart = head + MCP_TOOL_CALL_KEY.length;
        while (payloadStart < content.length && /\s/.test(content[payloadStart])) {
            payloadStart += 1;
        }
        if (content[payloadStart] === ":") {
            payloadStart += 1;
        }
        while (payloadStart < content.length && /\s/.test(content[payloadStart])) {
            payloadStart += 1;
        }

        return { commentStart, payloadStart };
    }

    return null;
}

function extractMcpToolCallComments(content: string): ParsedMcpToolCallComment[] {
    const comments: ParsedMcpToolCallComment[] = [];
    let cursor = 0;

    while (cursor < content.length) {
        const startInfo = findNextMcpToolCallStart(content, cursor);
        if (!startInfo) {
            break;
        }
        const start = startInfo.commentStart;
        const payloadStart = startInfo.payloadStart;
        let firstNonWhitespace = payloadStart;
        while (
            firstNonWhitespace < content.length &&
            /\s/.test(content[firstNonWhitespace])
        ) {
            firstNonWhitespace += 1;
        }

        let end = content.length;
        let complete = false;
        let parsedData: ToolCallData = {};

        const jsonEnd = findJsonObjectEnd(content, firstNonWhitespace);
        if (jsonEnd !== null) {
            const rawJson = content.slice(firstNonWhitespace, jsonEnd + 1);
            parsedData = parseToolCallPayload(rawJson) ?? parsePartialToolCallPayload(rawJson);
            const commentClose = content.indexOf("-->", jsonEnd + 1);
            if (commentClose !== -1) {
                end = commentClose + 3;
                complete = true;
            } else {
                end = content.length;
            }
        } else {
            const commentClose = content.indexOf("-->", payloadStart);
            if (commentClose !== -1) {
                const rawPayload = content.slice(payloadStart, commentClose);
                parsedData = parseToolCallPayload(rawPayload) ?? parsePartialToolCallPayload(rawPayload);
                end = commentClose + 3;
                complete = true;
            } else {
                const rawPayload = content.slice(payloadStart);
                parsedData = parsePartialToolCallPayload(rawPayload);
                end = content.length;
            }
        }

        comments.push({
            start,
            end,
            complete,
            data: parsedData,
        });

        if (!complete) {
            break;
        }
        cursor = end;
    }

    return comments;
}

function extractMcpToolCallXmlTags(content: string): ParsedMcpToolCallComment[] {
    const tags: ParsedMcpToolCallComment[] = [];
    const openTag = "<mcp_tool_call";
    const closeTag = "</mcp_tool_call>";
    let cursor = 0;

    while (cursor < content.length) {
        const start = content.indexOf(openTag, cursor);
        if (start === -1) {
            break;
        }

        const openTagEnd = content.indexOf(">", start);
        if (openTagEnd === -1) {
            tags.push({
                start,
                end: content.length,
                complete: false,
                data: {},
            });
            break;
        }

        const closeStart = content.indexOf(closeTag, openTagEnd + 1);
        if (closeStart === -1) {
            const rawPayload = content.slice(openTagEnd + 1);
            tags.push({
                start,
                end: content.length,
                complete: false,
                data: parsePartialXmlToolCallPayload(rawPayload),
            });
            break;
        }

        const rawPayload = content.slice(openTagEnd + 1, closeStart);
        tags.push({
            start,
            end: closeStart + closeTag.length,
            complete: true,
            data: parsePartialXmlToolCallPayload(rawPayload),
        });

        cursor = closeStart + closeTag.length;
    }

    return tags;
}

function extractMcpToolCalls(content: string): ParsedMcpToolCallComment[] {
    const merged = [
        ...extractMcpToolCallComments(content),
        ...extractMcpToolCallXmlTags(content),
    ].sort((a, b) => a.start - b.start);

    if (merged.length <= 1) {
        return merged;
    }

    const deduped: ParsedMcpToolCallComment[] = [];
    let lastEnd = -1;
    for (const item of merged) {
        if (item.start < lastEnd) {
            continue;
        }
        deduped.push(item);
        lastEnd = item.end;
        if (!item.complete) {
            break;
        }
    }
    return deduped;
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
            const errorMessage = getErrorMessage(error) || '发送结果失败';
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
    const { conversationId, messageId, mcpToolCallStates, shiningMcpCallId, inlineInteractionItems } = context || {};

    const processContent = useCallback((
        markdownContent: string,
        fallbackElement: React.ReactElement
    ): React.ReactElement => {
        const mcpCalls = extractMcpToolCalls(markdownContent);

        const renderInlineInteractionGroup = (
            key: string,
            items: InlineInteractionItem[]
        ): React.ReactElement => (
            <div key={key} className="flex flex-col gap-4 pt-2">
                {items.map((item) => (
                    <React.Fragment key={item.key}>{item.content}</React.Fragment>
                ))}
            </div>
        );

        if (mcpCalls.length === 0) {
            if (!inlineInteractionItems || inlineInteractionItems.length === 0) {
                return fallbackElement;
            }
            return (
                <div>
                    {fallbackElement}
                    {renderInlineInteractionGroup("inline-tail-no-mcp", inlineInteractionItems)}
                </div>
            );
        }

        console.log(
            "[MCP] detected MCP_TOOL_CALL comments",
            mcpCalls.map((match) => ({
                complete: match.complete,
                call_id: match.data.call_id,
                server_name: match.data.server_name,
                tool_name: match.data.tool_name,
            })),
            { conversationId, messageId },
        );

        // 收集所有工具调用数据
        const toolCallDataList: ToolCallData[] = [];
        const toolCallIds: number[] = [];
        const renderedInlineKeys = new Set<string>();

        // 将注释替换为实际的 React 组件
        const parts: React.ReactNode[] = [];
        let lastIndex = 0;

        for (const [index, match] of mcpCalls.entries()) {
            const data = match.data;
            toolCallDataList.push(data);
            if (data.call_id) {
                toolCallIds.push(data.call_id);
            }

            const beforeComment = markdownContent.slice(lastIndex, match.start);

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
                    key={`mcp-${data.call_id ?? `tmp-${index}-${match.start}`}`}
                    serverName={data.server_name}
                    toolName={data.tool_name}
                    parameters={data.parameters ?? "{}"}
                    conversationId={conversationId}
                    messageId={messageId}
                    callId={data.call_id} // 传递 callId，如果存在的话
                    mcpToolCallStates={mcpToolCallStates} // 传递全局 MCP 状态
                    shiningMcpCallId={shiningMcpCallId}
                    isLastCall={isLastCall} // 是否是最后一个工具调用
                />
            );

            if (data.call_id && inlineInteractionItems && inlineInteractionItems.length > 0) {
                const matchedInlineItems = inlineInteractionItems.filter(
                    (item) => item.callId === data.call_id
                );
                if (matchedInlineItems.length > 0) {
                    matchedInlineItems.forEach((item) => renderedInlineKeys.add(item.key));
                    parts.push(
                        renderInlineInteractionGroup(
                            `inline-after-call-${data.call_id}-${index}`,
                            matchedInlineItems
                        )
                    );
                }
            }

            lastIndex = match.end;
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

        if (inlineInteractionItems && inlineInteractionItems.length > 0) {
            const remainingInlineItems = inlineInteractionItems.filter(
                (item) => !renderedInlineKeys.has(item.key)
            );
            if (remainingInlineItems.length > 0) {
                parts.push(
                    renderInlineInteractionGroup(
                        "inline-message-tail",
                        remainingInlineItems
                    )
                );
            }
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
    }, [
        remarkPlugins,
        rehypePlugins,
        markdownComponents,
        conversationId,
        messageId,
        mcpToolCallStates,
        shiningMcpCallId,
        inlineInteractionItems,
    ]);

    return { processContent };
};
