import type {
    ConversationWithMessages,
    Message,
    MCPToolCall,
    ToolCallData,
} from "@/data/Conversation";

// 导出选项接口
export interface ConversationExportOptions {
    includeSystemPrompt: boolean;    // 是否导出 system prompt
    includeReasoning: boolean;       // 是否导出 reasoning
    includeToolParams: boolean;      // 是否导出工具使用参数
    includeToolResults: boolean;     // 是否导出工具使用结果
}

/**
 * 导出数据接口
 */
export interface ExportData {
    conversation: ConversationWithMessages;
    toolCalls: MCPToolCall[];
}

/**
 * 解析 tool_calls_json 为 ToolCallData 数组
 */
export function parseToolCalls(toolCallsJson: string | null | undefined): ToolCallData[] {
    if (!toolCallsJson) return [];
    try {
        return JSON.parse(toolCallsJson) as ToolCallData[];
    } catch {
        return [];
    }
}

interface BranchMessage {
    id: number;
    created_time: Date | string;
    generation_group_id?: string | null;
    parent_group_id?: string | null;
}

/**
 * 仅保留最新分支（与后端 get_latest_branch_messages 逻辑一致）
 */
export function getLatestBranchMessages<T extends BranchMessage>(messages: T[]): T[] {
    if (messages.length === 0) {
        return [];
    }
    const ordered = [...messages].sort((a, b) => {
        const aTime = new Date(a.created_time).getTime();
        const bTime = new Date(b.created_time).getTime();
        if (aTime === bTime) {
            return a.id - b.id;
        }
        return aTime - bTime;
    });

    const result: T[] = [];
    for (const msg of ordered) {
        if (msg.parent_group_id) {
            const parentGroupId = msg.parent_group_id;
            const firstIndex = result.findIndex((item) => item.generation_group_id === parentGroupId);
            if (firstIndex >= 0) {
                result.splice(firstIndex);
            }
        }
        if (msg.generation_group_id) {
            const currentGroupId = msg.generation_group_id;
            for (let i = result.length - 1; i >= 0; i -= 1) {
                if (result[i].generation_group_id === currentGroupId) {
                    result.splice(i, 1);
                }
            }
        }
        result.push(msg);
    }
    return result;
}

export interface McpToolCallHint {
    call_id?: number;
    server_name?: string;
    tool_name?: string;
    parameters?: string;
}

const MCP_TOOL_CALL_COMMENT_REGEX = /<!--\s*MCP_TOOL_CALL:[\s\S]*?-->/g;
const MCP_TOOL_CALL_XML_REGEX = /<mcp_tool_call>[\s\S]*?<\/mcp_tool_call>/gi;

/**
 * 移除导出内容中的 MCP 工具调用标记
 */
export function stripMcpToolCallMarkers(content: string): string {
    if (!content) return "";
    return content
        .replace(MCP_TOOL_CALL_COMMENT_REGEX, "")
        .replace(MCP_TOOL_CALL_XML_REGEX, "")
        .replace(/\n{3,}/g, "\n\n");
}

/**
 * 提取 MCP 工具调用注释中的数据（用于导出）
 */
export function extractMcpToolCallHints(content: string): McpToolCallHint[] {
    if (!content) return [];
    const matches = content.matchAll(/<!--\s*MCP_TOOL_CALL:(.*?) -->/g);
    const results: McpToolCallHint[] = [];
    for (const match of matches) {
        const raw = (match[1] || "").trim();
        if (!raw) continue;
        try {
            const parsed = JSON.parse(raw) as McpToolCallHint;
            const hint: McpToolCallHint = {};
            if (typeof parsed.call_id === "number" && Number.isFinite(parsed.call_id)) {
                hint.call_id = parsed.call_id;
            } else if (typeof (parsed as { call_id?: unknown }).call_id === "string") {
                const numericId = Number.parseInt((parsed as { call_id?: string }).call_id || "", 10);
                if (Number.isFinite(numericId)) {
                    hint.call_id = numericId;
                }
            }
            if (typeof parsed.server_name === "string" && parsed.server_name.trim()) {
                hint.server_name = parsed.server_name.trim();
            }
            if (typeof parsed.tool_name === "string" && parsed.tool_name.trim()) {
                hint.tool_name = parsed.tool_name.trim();
            }
            if (typeof parsed.parameters === "string" && parsed.parameters.trim()) {
                hint.parameters = parsed.parameters.trim();
            }
            if (hint.call_id || hint.server_name || hint.tool_name || hint.parameters) {
                results.push(hint);
            }
        } catch {
            const numericId = Number.parseInt(raw, 10);
            if (Number.isFinite(numericId)) {
                results.push({ call_id: numericId });
            }
        }
    }
    return results;
}

/**
 * 尝试格式化 JSON 字符串，失败则返回原始内容
 */
export function formatJsonContent(content: string): string {
    const trimmed = content.trim();
    if (!trimmed) return "";
    try {
        const parsed = JSON.parse(trimmed);
        return JSON.stringify(parsed, null, 2);
    } catch {
        return content;
    }
}

/**
 * 关联工具调用与消息
 * 返回 messageId -> toolCalls[] 的映射
 */
export function mapToolCallsToMessages(
    toolCalls: MCPToolCall[],
): Map<number, MCPToolCall[]> {
    const map = new Map<number, MCPToolCall[]>();
    for (const tc of toolCalls) {
        if (tc.message_id) {
            if (!map.has(tc.message_id)) {
                map.set(tc.message_id, []);
            }
            map.get(tc.message_id)!.push(tc);
        }
    }
    return map;
}

/**
 * 清理文件名中的非法字符
 */
export function sanitizeFilename(filename: string): string {
    return filename
        .replace(/[<>:"/\\|?*]/g, '') // 移除非法字符
        .replace(/\s+/g, '_')          // 空格替换为下划线
        .slice(0, 200);                 // 限制长度
}

/**
 * 格式化日期为本地字符串
 */
export function formatDate(date: Date): string {
    return new Date(date).toLocaleString('zh-CN', {
        year: 'numeric',
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
        second: '2-digit',
    });
}

/**
 * 获取消息显示名称
 */
export function getMessageTypeLabel(messageType: string): string {
    const labels: Record<string, string> = {
        system: '系统提示',
        user: '用户',
        assistant: '助手',
        reasoning: '推理过程',
        response: '回复',
        error: '错误',
    };
    return labels[messageType] || messageType;
}

/**
 * 根据选项过滤消息
 */
export function filterMessages(
    messages: Message[],
    options: ConversationExportOptions,
): Message[] {
    return messages.filter((msg) => {
        // tool_result 消息始终不导出（避免与结构化工具结果重复）
        if (msg.message_type === 'tool_result') {
            return false;
        }
        // 子任务/系统注入的工具执行文本不作为对话导出内容
        if (msg.message_type === 'user' && msg.content?.startsWith('Tool execution results:\n')) {
            return false;
        }
        // system 消息
        if (msg.message_type === 'system') {
            return options.includeSystemPrompt;
        }
        // reasoning 消息
        if (msg.message_type === 'reasoning') {
            return options.includeReasoning;
        }
        // 其他消息类型（user, assistant, response, error）始终包含
        return true;
    });
}

/**
 * 格式化为 Markdown
 */
export function formatAsMarkdown(
    data: ExportData,
    options: ConversationExportOptions,
): string {
    const { conversation, toolCalls } = data;
    const { conversation: convInfo, messages } = conversation;

    const lines: string[] = [];

    // 标题
    lines.push(`# ${convInfo.name}\n`);

    // 元信息
    lines.push(`**助手**: ${convInfo.assistant_name}`);
    lines.push(`**创建时间**: ${formatDate(convInfo.created_time)}`);
    lines.push('');

    // 分隔线
    lines.push('---\n');

    // 构建工具调用映射
    const toolCallMap = mapToolCallsToMessages(toolCalls);
    const toolCallById = new Map<number, MCPToolCall>();
    for (const tc of toolCalls) {
        toolCallById.set(tc.id, tc);
    }

    // 按顺序遍历消息
    for (const message of messages) {
        const msgType = message.message_type;

        // 跳过不符合选项的消息
        if (msgType === 'tool_result') continue;
        if (msgType === 'user' && message.content?.startsWith('Tool execution results:\n')) continue;
        if (msgType === 'system' && !options.includeSystemPrompt) continue;
        if (msgType === 'reasoning' && !options.includeReasoning) continue;

        const label = getMessageTypeLabel(msgType);
        lines.push(`## ${label}\n`);

        // 消息内容
        const messageContent = message.content || "";
        const sanitizedContent = stripMcpToolCallMarkers(messageContent);
        const content = sanitizedContent.trim() || '(无内容)';
        lines.push(content);
        lines.push('');

        // 工具调用参数
        if (options.includeToolParams) {
            const toolParamBlocks: string[] = [];
            if (message.tool_calls_json) {
                const nativeToolCalls = parseToolCalls(message.tool_calls_json);
                for (const tc of nativeToolCalls) {
                    const parts = tc.fn_name.split('__');
                    const toolName = parts.length > 1 ? parts.slice(1).join('__') : tc.fn_name;
                    toolParamBlocks.push(`**${toolName}**:`);
                    toolParamBlocks.push('```json');
                    toolParamBlocks.push(JSON.stringify(tc.fn_arguments, null, 2));
                    toolParamBlocks.push('```\n');
                }
            } else if (toolCallMap.has(message.id)) {
                const mappedCalls = toolCallMap.get(message.id) || [];
                for (const tc of mappedCalls) {
                    toolParamBlocks.push(`**${tc.server_name} / ${tc.tool_name}**:`);
                    toolParamBlocks.push('```json');
                    toolParamBlocks.push(formatJsonContent(tc.parameters));
                    toolParamBlocks.push('```\n');
                }
            }
            if (toolParamBlocks.length > 0) {
                lines.push(`### 工具调用\n`);
                lines.push(...toolParamBlocks);
            }
        }

        // 工具执行结果
        if (options.includeToolResults && toolCallMap.has(message.id)) {
            const relatedCalls = (toolCallMap.get(message.id) || []).filter(
                (tc) => tc.status === 'success' && tc.result,
            );
            if (relatedCalls.length > 0) {
                lines.push(`### 工具执行结果\n`);
                for (const tc of relatedCalls) {
                    lines.push(`**${tc.server_name} / ${tc.tool_name}**:`);
                    lines.push('```json');
                    lines.push(formatJsonContent(tc.result ?? ""));
                    lines.push('```');
                    lines.push('');
                }
            }
        }

        lines.push('---\n');
    }

    return lines.join('\n');
}

/**
 * 转义 HTML 特殊字符
 */
export function escapeHtml(text: string): string {
    const htmlEntities: Record<string, string> = {
        '&': '&amp;',
        '<': '&lt;',
        '>': '&gt;',
        '"': '&quot;',
        "'": '&#39;',
    };
    return text.replace(/[&<>"']/g, (char) => htmlEntities[char] || char);
}

/**
 * 格式化消息内容为 HTML（用于 PDF/图片导出）
 */
export function formatMessageContentAsHtml(content: string): string {
    // 简单处理：将换行转换为 <br>，代码块用 <pre> 包裹
    let html = escapeHtml(stripMcpToolCallMarkers(content));

    // 处理代码块 ```lang ... ```
    html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, lang, code) => {
        return `<pre><code class="language-${lang}">${code}</code></pre>`;
    });

    // 处理行内代码 `...`
    html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

    // 处理换行
    html = html.replace(/\n/g, '<br>');

    return html;
}

/**
 * 生成导出内容的 HTML（用于 PDF/图片导出）
 */
export function generateExportHtml(
    data: ExportData,
    options: ConversationExportOptions,
): string {
    const { conversation } = data;
    const { conversation: convInfo, messages } = conversation;

    // 过滤消息
    const filteredMessages = filterMessages(messages, options);
    const toolCallById = new Map<number, MCPToolCall>();
    for (const tc of data.toolCalls) {
        toolCallById.set(tc.id, tc);
    }

    let html = `
<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <title>${escapeHtml(convInfo.name)}</title>
    <style>
        body {
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif;
            line-height: 1.6;
            color: #333;
            max-width: 800px;
            margin: 0 auto;
            padding: 20px;
            background: #fff;
        }
        h1 {
            font-size: 24px;
            margin-bottom: 10px;
            color: #111;
        }
        .meta {
            color: #666;
            font-size: 14px;
            margin-bottom: 20px;
        }
        .message {
            margin-bottom: 24px;
            padding: 16px;
            border-radius: 8px;
            background: #f5f5f5;
        }
        .message-header {
            font-weight: 600;
            margin-bottom: 8px;
            color: #333;
        }
        .message-content {
            white-space: pre-wrap;
            word-break: break-word;
        }
        pre {
            background: #1e1e1e;
            color: #d4d4d4;
            padding: 12px;
            border-radius: 4px;
            overflow-x: auto;
            margin: 8px 0;
        }
        code {
            font-family: "Consolas", "Monaco", monospace;
            font-size: 13px;
        }
        .tool-call {
            margin-top: 12px;
            padding: 8px;
            background: #e8f4f8;
            border-left: 3px solid #007acc;
            border-radius: 4px;
        }
        .tool-result {
            margin-top: 12px;
            padding: 8px;
            background: #f0f8f0;
            border-left: 3px solid #28a745;
            border-radius: 4px;
        }
        .section-label {
            font-size: 12px;
            font-weight: 600;
            margin-bottom: 4px;
            color: #666;
        }
    </style>
</head>
<body>
    <h1>${escapeHtml(convInfo.name)}</h1>
    <div class="meta">
        助手: ${escapeHtml(convInfo.assistant_name)} |
        创建时间: ${formatDate(convInfo.created_time)}
    </div>
`;

    for (const message of filteredMessages) {
        const label = getMessageTypeLabel(message.message_type);
        const messageContent = message.content || "";
        const sanitizedContent = stripMcpToolCallMarkers(messageContent);

        html += `
    <div class="message">
        <div class="message-header">${escapeHtml(label)}</div>
        <div class="message-content">${escapeHtml(sanitizedContent)}</div>
`;

        // 工具调用参数
        if (options.includeToolParams) {
            const nativeToolCalls = message.tool_calls_json
                ? parseToolCalls(message.tool_calls_json)
                : [];
            if (nativeToolCalls.length > 0) {
                for (const tc of nativeToolCalls) {
                    const parts = tc.fn_name.split('__');
                    const toolName = parts.length > 1 ? parts.slice(1).join('__') : tc.fn_name;
                    html += `
        <div class="tool-call">
            <div class="section-label">工具调用: ${escapeHtml(toolName)}</div>
            <pre><code>${escapeHtml(JSON.stringify(tc.fn_arguments, null, 2))}</code></pre>
        </div>
`;
                }
            } else {
                const mcpHints = extractMcpToolCallHints(messageContent);
                for (const hint of mcpHints) {
                    const fromDb = hint.call_id ? toolCallById.get(hint.call_id) : undefined;
                    const serverName = fromDb?.server_name ?? hint.server_name ?? "unknown";
                    const toolName = fromDb?.tool_name ?? hint.tool_name ?? "unknown";
                    const paramsText = fromDb?.parameters ?? hint.parameters ?? "{}";
                    html += `
        <div class="tool-call">
            <div class="section-label">工具调用: ${escapeHtml(serverName)} / ${escapeHtml(toolName)}</div>
            <pre><code>${escapeHtml(formatJsonContent(paramsText))}</code></pre>
        </div>
`;
                }
            }
        }

        // 工具执行结果
        if (options.includeToolResults && toolCallMap.has(message.id)) {
            const relatedCalls = (toolCallMap.get(message.id) || []).filter(
                (tc) => tc.status === 'success' && tc.result,
            );
            for (const tc of relatedCalls) {
                html += `
        <div class="tool-result">
            <div class="section-label">工具结果: ${escapeHtml(tc.server_name)} / ${escapeHtml(tc.tool_name)}</div>
            <pre><code>${escapeHtml(formatJsonContent(tc.result ?? ""))}</code></pre>
        </div>
`;
            }
        }

        html += `</div>
`;
    }

    html += `
</body>
</html>
`;

    return html;
}
