import React, { useCallback } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import McpToolCallRenderer from '@/components/McpToolCallRenderer';
import { MCPToolCallUpdateEvent } from '@/data/Conversation';
import { customUrlTransform } from '@/constants/markdown';
import type { InlineInteractionItem } from '@/components/ConversationUI';
import { parsePreviewCodeStreamingState, type PreviewCodeStreamingState } from '@/utils/previewCode';

interface McpProcessorOptions {
    remarkPlugins: readonly any[];
    rehypePlugins: readonly any[];
    markdownComponents: Components;
}

interface ProcessorContext {
    conversationId?: number;
    messageId?: number;
    isLastMessage?: boolean;
    mcpToolCallStates?: Map<number, MCPToolCallUpdateEvent>;
    shiningMcpCallId?: number | null;
    inlineInteractionItems?: InlineInteractionItem[];
}

interface ToolCallData {
    server_name?: string;
    tool_name?: string;
    parameters?: string;
    call_id?: number;
    llm_call_id?: string;
    status?: MCPToolCallUpdateEvent["status"];
    error?: string;
    isStreaming?: boolean;  // 流式工具调用（参数可能不完整）
    fn_arguments?: string;  // 流式工具调用的原始参数
    preview_state?: PreviewCodeStreamingState;
}

interface ParsedMcpToolCallComment {
    start: number;
    end: number;
    complete: boolean;
    data: ToolCallData;
}

const MCP_TOOL_CALL_KEY = "MCP_TOOL_CALL";
const MCP_TOOL_CALL_STREAMING_KEY = "MCP_TOOL_CALL_STREAMING";

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
    const llmCallIdRaw = value.llm_call_id;
    const statusRaw = value.status;
    const status =
        statusRaw === "pending" ||
            statusRaw === "executing" ||
            statusRaw === "success" ||
            statusRaw === "failed" ||
            statusRaw === "unknown"
            ? statusRaw
            : undefined;
    const setupError = typeof value.setup_error === "string" ? value.setup_error : undefined;
    const error = typeof value.error === "string" ? value.error : setupError;
    return {
        server_name: typeof value.server_name === "string" ? value.server_name : undefined,
        tool_name: typeof value.tool_name === "string" ? value.tool_name : undefined,
        parameters: typeof value.parameters === "string" ? value.parameters : undefined,
        call_id: callId,
        llm_call_id: typeof llmCallIdRaw === "string" ? llmCallIdRaw : undefined,
        status: status ?? (setupError ? "failed" : undefined),
        error,
        fn_arguments: typeof value.fn_arguments === "string" ? value.fn_arguments : undefined,
        preview_state: parsePreviewCodeStreamingState(value.preview_state) ?? undefined,
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
    const llmCallIdMatch = rawPayload.match(/"llm_call_id"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (llmCallIdMatch) {
        result.llm_call_id = decodeJsonString(llmCallIdMatch[1]);
    }
    const statusMatch = rawPayload.match(/"status"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (statusMatch) {
        const status = decodeJsonString(statusMatch[1]);
        if (
            status === "pending" ||
            status === "executing" ||
            status === "success" ||
            status === "failed" ||
            status === "unknown"
        ) {
            result.status = status;
        }
    }
    const errorMatch = rawPayload.match(/"error"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (errorMatch) {
        result.error = decodeJsonString(errorMatch[1]);
    }
    const setupErrorMatch = rawPayload.match(/"setup_error"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (setupErrorMatch) {
        result.status = result.status ?? "failed";
        result.error = result.error ?? decodeJsonString(setupErrorMatch[1]);
    }
    const fnArgsMatch = rawPayload.match(/"fn_arguments"\s*:\s*"((?:\\.|[^"\\])*)"/);
    if (fnArgsMatch) {
        result.fn_arguments = decodeJsonString(fnArgsMatch[1]);
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
): { commentStart: number; payloadStart: number; isStreaming: boolean } | null {
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

        // 优先检查更长的 STREAMING key（避免被短 key 误匹配）
        let matchedKey: string | null = null;
        let isStreaming = false;
        if (content.startsWith(MCP_TOOL_CALL_STREAMING_KEY, head)) {
            matchedKey = MCP_TOOL_CALL_STREAMING_KEY;
            isStreaming = true;
        } else if (content.startsWith(MCP_TOOL_CALL_KEY, head)) {
            matchedKey = MCP_TOOL_CALL_KEY;
        }

        if (!matchedKey) {
            cursor = commentStart + 4;
            continue;
        }

        let payloadStart = head + matchedKey.length;
        while (payloadStart < content.length && /\s/.test(content[payloadStart])) {
            payloadStart += 1;
        }
        if (content[payloadStart] === ":") {
            payloadStart += 1;
        }
        while (payloadStart < content.length && /\s/.test(content[payloadStart])) {
            payloadStart += 1;
        }

        return { commentStart, payloadStart, isStreaming };
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
        const isStreaming = startInfo.isStreaming;
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

        // 标记流式工具调用
        if (isStreaming) {
            parsedData.isStreaming = true;
            // 流式标记中使用 fn_arguments 字段
            if (!parsedData.parameters && parsedData.fn_arguments) {
                parsedData.parameters = parsedData.fn_arguments;
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
            const data = parsePartialXmlToolCallPayload(rawPayload);
            // Incomplete XML tag → the model is still generating this tool call
            data.isStreaming = true;
            tags.push({
                start,
                end: content.length,
                complete: false,
                data,
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
    let sealed = false;
    for (const item of merged) {
        if (item.start < lastEnd) {
            // This item is shadowed by the previous one's range.
            // When an incomplete XML tag shadows a streaming HTML comment,
            // merge the richer streaming data (preview_state, llm_call_id, etc.)
            // into the XML entry so the component receives streaming context.
            if (
                deduped.length > 0
                && !deduped[deduped.length - 1].complete
                && item.data.isStreaming
            ) {
                const prev = deduped[deduped.length - 1].data;
                prev.isStreaming = true;
                if (item.data.preview_state && !prev.preview_state) {
                    prev.preview_state = item.data.preview_state;
                }
                if (item.data.fn_arguments && !prev.fn_arguments) {
                    prev.fn_arguments = item.data.fn_arguments;
                }
                if (item.data.llm_call_id && !prev.llm_call_id) {
                    prev.llm_call_id = item.data.llm_call_id;
                }
            }
            continue;
        }
        if (sealed) {
            break;
        }
        deduped.push(item);
        lastEnd = item.end;
        if (!item.complete) {
            // Don't break — continue iterating so shadowed streaming comments
            // within the incomplete range can still merge their data.
            sealed = true;
        }
    }
    return deduped;
}

function getMcpToolCallKey(
    data: ToolCallData,
    messageId: number | undefined,
    index: number,
): string {
    if (data.llm_call_id) {
        return `mcp-call-${data.llm_call_id}`;
    }
    if (data.call_id) {
        return `mcp-call-${data.call_id}`;
    }
    if (data.isStreaming) {
        return `mcp-stream-${messageId ?? "message"}-${index}-${data.server_name ?? "server"}-${data.tool_name ?? "tool"}`;
    }
    return `mcp-slot-${messageId ?? "message"}-${index}-${data.server_name ?? "server"}-${data.tool_name ?? "tool"}`;
}

export const useMcpToolCallProcessor = (options: McpProcessorOptions, context?: ProcessorContext) => {
    const { remarkPlugins, rehypePlugins, markdownComponents } = options;
    const {
        conversationId,
        messageId,
        mcpToolCallStates,
        shiningMcpCallId,
        isLastMessage,
        inlineInteractionItems,
    } = context || {};

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

        const renderedInlineKeys = new Set<string>();

        // 将注释替换为实际的 React 组件
        const parts: React.ReactNode[] = [];
        let lastIndex = 0;

        for (const [index, match] of mcpCalls.entries()) {
            const data = match.data;

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
            const toolCallKey = getMcpToolCallKey(data, messageId, index);
            parts.push(
                <McpToolCallRenderer
                    key={toolCallKey}
                    serverName={data.server_name}
                    toolName={data.tool_name}
                    parameters={data.parameters ?? "{}"}
                    llmCallId={data.llm_call_id}
                    status={data.status}
                    error={data.error}
                    conversationId={conversationId}
                    messageId={messageId}
                    callId={data.call_id} // 传递 callId，如果存在的话
                    mcpToolCallStates={mcpToolCallStates} // 传递全局 MCP 状态
                    shiningMcpCallId={shiningMcpCallId}
                    isLastCall={isLastCall} // 是否是最后一个工具调用
                    isStreaming={data.isStreaming} // 流式工具调用标记
                    streamingPreviewState={data.preview_state}
                    isLastMessage={isLastMessage}
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

        return <div>{parts}</div>;
    }, [
        remarkPlugins,
        rehypePlugins,
        markdownComponents,
        conversationId,
        messageId,
        mcpToolCallStates,
        shiningMcpCallId,
        isLastMessage,
        inlineInteractionItems,
    ]);

    return { processContent };
};
