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

    // 按顺序遍历消息
    for (const message of messages) {
        const msgType = message.message_type;

        // 跳过不符合选项的消息
        if (msgType === 'system' && !options.includeSystemPrompt) continue;
        if (msgType === 'reasoning' && !options.includeReasoning) continue;

        const label = getMessageTypeLabel(msgType);
        lines.push(`## ${label}\n`);

        // 消息内容
        const content = message.content?.trim() || '(无内容)';
        lines.push(content);
        lines.push('');

        // 工具调用参数
        if (options.includeToolParams && message.tool_calls_json) {
            const toolCalls = parseToolCalls(message.tool_calls_json);
            if (toolCalls.length > 0) {
                lines.push(`### 工具调用\n`);
                for (const tc of toolCalls) {
                    // 解析 fn_name (格式: server__tool)
                    const parts = tc.fn_name.split('__');
                    const toolName = parts.length > 1 ? parts.slice(1).join('__') : tc.fn_name;
                    lines.push(`**${toolName}**:`);
                    lines.push('```json');
                    lines.push(JSON.stringify(tc.fn_arguments, null, 2));
                    lines.push('```\n');
                }
            }
        }

        // 工具执行结果
        if (options.includeToolResults && toolCallMap.has(message.id)) {
            const relatedCalls = toolCallMap.get(message.id)!;
            if (relatedCalls.length > 0) {
                lines.push(`### 工具执行结果\n`);
                for (const tc of relatedCalls) {
                    lines.push(`**${tc.tool_name}** (${tc.status}):`);
                    if (tc.status === 'success' && tc.result) {
                        lines.push('```json');
                        try {
                            const result = JSON.parse(tc.result);
                            lines.push(JSON.stringify(result, null, 2));
                        } catch {
                            lines.push(tc.result);
                        }
                        lines.push('```');
                    } else if (tc.status === 'failed' && tc.error) {
                        lines.push(`\n错误: ${tc.error}`);
                    }
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
    let html = escapeHtml(content);

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

    // 构建工具调用映射
    const toolCallMap = mapToolCallsToMessages(data.toolCalls);

    for (const message of filteredMessages) {
        const label = getMessageTypeLabel(message.message_type);

        html += `
    <div class="message">
        <div class="message-header">${escapeHtml(label)}</div>
        <div class="message-content">${escapeHtml(message.content || '')}</div>
`;

        // 工具调用参数
        if (options.includeToolParams && message.tool_calls_json) {
            const toolCalls = parseToolCalls(message.tool_calls_json);
            for (const tc of toolCalls) {
                const parts = tc.fn_name.split('__');
                const toolName = parts.length > 1 ? parts.slice(1).join('__') : tc.fn_name;
                html += `
        <div class="tool-call">
            <div class="section-label">工具调用: ${escapeHtml(toolName)}</div>
            <pre><code>${JSON.stringify(tc.fn_arguments, null, 2)}</code></pre>
        </div>
`;
            }
        }

        // 工具执行结果
        if (options.includeToolResults && toolCallMap.has(message.id)) {
            const relatedCalls = toolCallMap.get(message.id)!;
            for (const tc of relatedCalls) {
                html += `
        <div class="tool-result">
            <div class="section-label">工具结果: ${escapeHtml(tc.tool_name)} (${tc.status})</div>
`;
                if (tc.status === 'success' && tc.result) {
                    html += `<pre><code>${escapeHtml(tc.result)}</code></pre>`;
                } else if (tc.status === 'failed' && tc.error) {
                    html += `<div style="color: #d32f2f;">错误: ${escapeHtml(tc.error)}</div>`;
                }
                html += `</div>
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
