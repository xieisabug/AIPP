export const LARGE_TOOL_RESULT_CHAR_THRESHOLD = 12000;
export const LARGE_TOOL_RESULT_LINE_THRESHOLD = 240;
export const LARGE_MCP_PAYLOAD_CHAR_THRESHOLD = 5000;
export const LARGE_MESSAGE_PREVIEW_CHAR_LIMIT = 5000;
export const LARGE_MESSAGE_PREVIEW_LINE_LIMIT = 40;
export const LARGE_MESSAGE_PREVIEW_HEIGHT_ESTIMATE = 120;

const MCP_TOOL_CALL_COMMENT_PATTERN =
    /<!--\s*MCP_TOOL_CALL(?:_STREAMING)?\:([\s\S]*?)-->/g;
const MCP_TOOL_CALL_XML_PATTERN =
    /<mcp_tool_call[\s\S]*?<\/mcp_tool_call>/gi;

export interface LargeMessagePreviewStats {
    lineCount: number;
    payloadCharCount: number;
    contentHash?: string;
    reason: "none" | "tool_result" | "mcp_payload";
    shouldPreview: boolean;
    summary: string;
    previewText: string;
}

export type LargeMessagePreviewMetadata = Omit<
    LargeMessagePreviewStats,
    "reason"
> & {
    reason: "tool_result" | "mcp_payload";
};

interface McpToolCallPreviewInfo {
    callId?: string | number;
    toolName?: string;
    payloadLength: number;
    segmentLength: number;
}

function parseMcpToolCallPreviewInfo(
    segmentBody: string,
    segmentLength: number,
): McpToolCallPreviewInfo {
    try {
        const parsed = JSON.parse(segmentBody.trim()) as Record<string, unknown>;
        const parameters = parsed.parameters;
        const payloadLength = typeof parameters === "string"
            ? parameters.length
            : parameters == null
                ? segmentLength
                : JSON.stringify(parameters).length;
        const toolName = typeof parsed.tool_name === "string"
            ? parsed.tool_name
            : typeof parsed.name === "string"
                ? parsed.name
                : undefined;

        return {
            callId: typeof parsed.call_id === "string" || typeof parsed.call_id === "number"
                ? parsed.call_id
                : undefined,
            toolName,
            payloadLength,
            segmentLength,
        };
    } catch {
        return {
            payloadLength: segmentLength,
            segmentLength,
        };
    }
}

function collectMcpToolCallPreviewInfo(content: string): McpToolCallPreviewInfo[] {
    const infos: McpToolCallPreviewInfo[] = [];

    for (const match of content.matchAll(MCP_TOOL_CALL_COMMENT_PATTERN)) {
        infos.push(parseMcpToolCallPreviewInfo(match[1] ?? "", match[0].length));
    }

    for (const match of content.matchAll(MCP_TOOL_CALL_XML_PATTERN)) {
        infos.push({
            payloadLength: match[0].length,
            segmentLength: match[0].length,
        });
    }

    return infos;
}

function hasMcpToolCall(content: string): boolean {
    return content.includes("MCP_TOOL_CALL") || content.includes("<mcp_tool_call");
}

function buildMcpPayloadPreview(
    lineCount: number,
    largestPayload: McpToolCallPreviewInfo,
): LargeMessagePreviewStats {
    const summaryParts = ["MCP 工具调用"];
    if (largestPayload.toolName) {
        summaryParts.push(`工具: ${largestPayload.toolName}`);
    }
    if (largestPayload.callId != null) {
        summaryParts.push(`Call ID: ${largestPayload.callId}`);
    }

    const summary = summaryParts.join(" · ");
    return {
        lineCount,
        payloadCharCount: largestPayload.payloadLength,
        reason: "mcp_payload",
        shouldPreview: true,
        summary,
        previewText: "",
    };
}

export function getLargeMessagePreviewStats(
    content: string,
    messageType: string,
    previewMetadata?: LargeMessagePreviewMetadata | null,
): LargeMessagePreviewStats {
    if (previewMetadata?.shouldPreview) {
        return previewMetadata;
    }

    const lineCount = content.length > 0 ? content.split(/\r?\n/).length : 1;

    if (messageType === "tool_result") {
        return {
            lineCount,
            payloadCharCount: content.length,
            reason: "tool_result",
            shouldPreview: true,
            summary: "工具结果",
            previewText: "",
        };
    }

    if (messageType === "response") {
        const containsMcpToolCall = hasMcpToolCall(content);
        const largestPayload = collectMcpToolCallPreviewInfo(content)
            .reduce<McpToolCallPreviewInfo | null>(
                (largest, payload) =>
                    largest == null || payload.payloadLength > largest.payloadLength
                        ? payload
                        : largest,
                null,
            );

        if (containsMcpToolCall && largestPayload) {
            return buildMcpPayloadPreview(lineCount, largestPayload);
        }

        if (containsMcpToolCall) {
            return {
                lineCount,
                payloadCharCount: content.length,
                reason: "mcp_payload",
                shouldPreview: true,
                summary: "MCP 工具调用",
                previewText: "",
            };
        }
    }

    return {
        lineCount,
        payloadCharCount: content.length,
        reason: "none",
        shouldPreview: false,
        summary: "",
        previewText: content,
    };
}

export function shouldUseLargeMessagePreview({
    content,
    isLastMessage,
    isStreaming,
    messageType,
    previewMetadata,
}: {
    content: string;
    isLastMessage: boolean;
    isStreaming: boolean;
    messageType: string;
    previewMetadata?: LargeMessagePreviewMetadata | null;
}): boolean {
    if (isStreaming || isLastMessage) {
        return false;
    }

    return getLargeMessagePreviewStats(
        content,
        messageType,
        previewMetadata,
    ).shouldPreview;
}

export function formatLargeMessageCount(value: number): string {
    return new Intl.NumberFormat("zh-CN").format(value);
}
