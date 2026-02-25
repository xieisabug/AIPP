import React from "react";
import { createRoot } from "react-dom/client";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import remarkCodeBlockMeta from "@/react-markdown/remarkCodeBlockMeta";
import type { ExportData, ConversationExportOptions } from "@/utils/exportFormatters";
import { parseToolCalls, mapToolCallsToMessages, stripMcpToolCallMarkers, extractMcpToolCallHints, formatJsonContent } from "@/utils/exportFormatters";
import { resolveCodeBlockMeta } from "@/react-markdown/remarkCodeBlockMeta";

// 导出专用的颜色方案 - 使用纯 RGB 值避免 oklch 兼容性问题
const exportColors = {
    light: {
        background: "#ffffff",
        foreground: "#0a0a0b",
        muted: "#f4f4f5",
        mutedForeground: "#71717a",
        border: "#e4e4e7",
        card: "#ffffff",
        userBubble: "#f4f4f5",
        assistantBubble: "#ffffff",
        success: "#22c55e",
        error: "#ef4444",
        warning: "#f59e0b",
        link: "#2563eb",
        codeBg: "#f4f4f5",
        codeText: "#0a0a0b",
    },
    dark: {
        background: "#0a0a0b",
        foreground: "#fafafa",
        muted: "#27272a",
        mutedForeground: "#a1a1aa",
        border: "#27272a",
        card: "#18181b",
        userBubble: "#27272a",
        assistantBubble: "#18181b",
        success: "#22c55e",
        error: "#ef4444",
        warning: "#f59e0b",
        link: "#60a5fa",
        codeBg: "#27272a",
        codeText: "#fafafa",
    },
};

type ColorScheme = typeof exportColors.light;

/**
 * 导出专用的 Markdown 组件配置
 * 使用内联样式避免 CSS 变量和 oklch 颜色
 */
const createExportMarkdownComponents = (colors: ColorScheme) => ({
    h1: ({ children, ...props }: any) => (
        <h1 style={{ fontSize: "1.875em", fontWeight: 600, margin: "1em 0 0.5em", color: colors.foreground }} {...props}>{children}</h1>
    ),
    h2: ({ children, ...props }: any) => (
        <h2 style={{ fontSize: "1.5em", fontWeight: 600, margin: "1em 0 0.5em", color: colors.foreground }} {...props}>{children}</h2>
    ),
    h3: ({ children, ...props }: any) => (
        <h3 style={{ fontSize: "1.25em", fontWeight: 600, margin: "1em 0 0.5em", color: colors.foreground }} {...props}>{children}</h3>
    ),
    h4: ({ children, ...props }: any) => (
        <h4 style={{ fontSize: "1.1em", fontWeight: 600, margin: "0.75em 0 0.5em", color: colors.foreground }} {...props}>{children}</h4>
    ),
    p: ({ children, ...props }: any) => (
        <p style={{ margin: "0.75em 0", color: colors.foreground, lineHeight: 1.7 }} {...props}>{children}</p>
    ),
    ul: ({ children, ...props }: any) => (
        <ul style={{ margin: "0.75em 0", paddingLeft: "1.5em", color: colors.foreground }} {...props}>{children}</ul>
    ),
    ol: ({ children, ...props }: any) => (
        <ol style={{ margin: "0.75em 0", paddingLeft: "1.5em", color: colors.foreground }} {...props}>{children}</ol>
    ),
    li: ({ children, ...props }: any) => (
        <li style={{ margin: "0.25em 0", color: colors.foreground }} {...props}>{children}</li>
    ),
    a: ({ children, href, ...props }: any) => (
        <a style={{ color: colors.link, textDecoration: "underline" }} href={href} {...props}>{children}</a>
    ),
    blockquote: ({ children, ...props }: any) => (
        <blockquote style={{
            margin: "1em 0",
            paddingLeft: "1em",
            borderLeft: `4px solid ${colors.border}`,
            color: colors.mutedForeground,
            fontStyle: "italic"
        }} {...props}>{children}</blockquote>
    ),
    code: ({ className, children, node, ...props }: any) => {
        const isBlock = className?.includes("language-");
        const meta = resolveCodeBlockMeta(props as Record<string, unknown>, node);
        const dataLanguage = typeof (props as Record<string, unknown>)["data-language"] === "string"
            ? (props as Record<string, unknown>)["data-language"] as string
            : undefined;
        const language = className?.includes("language-")
            ? className?.replace("language-", "")
            : dataLanguage;
        const metaLabel = meta
            ? [meta.title || meta.filename, meta.line ? `line ${meta.line}` : null, meta.highlight ? `highlight ${meta.highlight}` : null]
                .filter(Boolean)
                .join(" · ")
            : null;
        if (isBlock || meta || dataLanguage) {
            return (
                <pre style={{
                    backgroundColor: colors.codeBg,
                    padding: "12px",
                    borderRadius: "8px",
                    overflow: "auto",
                    margin: "0.75em 0",
                    border: `1px solid ${colors.border}`,
                }}>
                    {metaLabel && (
                        <div style={{ fontSize: "11px", color: colors.mutedForeground, marginBottom: "6px", fontFamily: "Consolas, Monaco, \"Courier New\", monospace" }}>
                            {metaLabel}
                        </div>
                    )}
                    <code style={{
                        fontFamily: 'Consolas, Monaco, "Courier New", monospace',
                        fontSize: "13px",
                        color: colors.codeText,
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-word",
                    }} data-language={language} {...props}>{children}</code>
                </pre>
            );
        }
        return (
            <code style={{
                backgroundColor: colors.codeBg,
                padding: "2px 6px",
                borderRadius: "4px",
                fontFamily: 'Consolas, Monaco, "Courier New", monospace',
                fontSize: "0.9em",
                color: colors.codeText,
            }} {...props}>{children}</code>
        );
    },
    pre: ({ children, ...props }: any) => (
        <div {...props}>{children}</div>
    ),
    table: ({ children, ...props }: any) => (
        <table style={{
            borderCollapse: "collapse",
            width: "100%",
            margin: "1em 0",
            border: `1px solid ${colors.border}`,
        }} {...props}>{children}</table>
    ),
    th: ({ children, ...props }: any) => (
        <th style={{
            border: `1px solid ${colors.border}`,
            padding: "8px 12px",
            backgroundColor: colors.muted,
            fontWeight: 600,
            textAlign: "left",
            color: colors.foreground,
        }} {...props}>{children}</th>
    ),
    td: ({ children, ...props }: any) => (
        <td style={{
            border: `1px solid ${colors.border}`,
            padding: "8px 12px",
            color: colors.foreground,
        }} {...props}>{children}</td>
    ),
    hr: (props: any) => (
        <hr style={{ border: "none", borderTop: `1px solid ${colors.border}`, margin: "1.5em 0" }} {...props} />
    ),
    strong: ({ children, ...props }: any) => (
        <strong style={{ fontWeight: 600, color: colors.foreground }} {...props}>{children}</strong>
    ),
    em: ({ children, ...props }: any) => (
        <em style={{ fontStyle: "italic", color: colors.foreground }} {...props}>{children}</em>
    ),
});

/**
 * 导出专用的 Markdown 渲染器
 */
const ExportMarkdown: React.FC<{ children: string; colors: ColorScheme }> = ({ children, colors }) => {
    const components = createExportMarkdownComponents(colors);
    return (
        <ReactMarkdown
            remarkPlugins={[remarkGfm, remarkCodeBlockMeta]}
            components={components}
        >
            {children}
        </ReactMarkdown>
    );
};

interface ConversationExportRendererProps {
    data: ExportData;
    options: ConversationExportOptions;
    conversationName: string;
    assistantName: string;
    createdTime: Date;
    isDarkMode?: boolean;
}

/**
 * 对话导出渲染器 - 用于 PDF/图片导出
 * 使用内联样式避免 oklch 颜色函数兼容性问题
 * 样式模仿实际对话界面：用户消息靠右，助手消息靠左
 */
const ConversationExportRenderer: React.FC<ConversationExportRendererProps> = ({
    data,
    options,
    conversationName,
    assistantName,
    createdTime,
    isDarkMode = false,
}) => {
    const { conversation, toolCalls } = data;
    const { messages } = conversation;
    const colors = isDarkMode ? exportColors.dark : exportColors.light;

    // 构建工具调用映射
    const toolCallMap = mapToolCallsToMessages(toolCalls);
    const toolCallById = new Map<number, (typeof toolCalls)[number]>();
    for (const tc of toolCalls) {
        toolCallById.set(tc.id, tc);
    }

    // 过滤消息
    const filteredMessages = messages.filter((msg) => {
        if (msg.message_type === "tool_result") return false;
        if (msg.message_type === "user" && msg.content?.startsWith("Tool execution results:\n")) return false;
        if (msg.message_type === "system") return options.includeSystemPrompt;
        if (msg.message_type === "reasoning") return options.includeReasoning;
        return true;
    });

    const formatDate = (date: Date) => {
        return new Date(date).toLocaleString("zh-CN", {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
        });
    };

    // 通用样式
    const styles = {
        container: {
            width: "100%",
            backgroundColor: colors.background,
            color: colors.foreground,
            padding: "24px",
            fontFamily: '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Microsoft YaHei", sans-serif',
            lineHeight: "1.6",
            boxSizing: "border-box" as const,
        },
        header: {
            marginBottom: "24px",
            paddingBottom: "16px",
            borderBottom: `1px solid ${colors.border}`,
        },
        title: {
            fontSize: "24px",
            fontWeight: 600,
            margin: "0 0 8px 0",
            color: colors.foreground,
        },
        subtitle: {
            fontSize: "14px",
            color: colors.mutedForeground,
            margin: 0,
        },
        messageWrapper: {
            display: "flex",
            flexDirection: "column" as const,
            marginBottom: "16px",
        },
        userMessageWrapper: {
            alignItems: "flex-end" as const,
        },
        assistantMessageWrapper: {
            alignItems: "flex-start" as const,
        },
        messageBubble: {
            maxWidth: "75%",
            padding: "8px 20px",
            borderRadius: "16px",
            boxSizing: "border-box" as const,
        },
        userBubble: {
            backgroundColor: colors.userBubble,
            border: `1px solid ${colors.border}`,
        },
        assistantBubble: {
            backgroundColor: colors.assistantBubble,
            border: `1px solid ${colors.border}`,
        },
        systemBubble: {
            backgroundColor: colors.muted,
            border: `1px solid ${colors.border}`,
            maxWidth: "100%",
        },
        reasoningBubble: {
            backgroundColor: colors.muted,
            border: `1px dashed ${colors.border}`,
            opacity: 0.9,
        },
        messageLabel: {
            fontSize: "12px",
            color: colors.mutedForeground,
            marginBottom: "4px",
            fontWeight: 500,
        },
        prose: {
            fontSize: "15px",
            lineHeight: "1.7",
            color: colors.foreground,
        },
        toolCallBox: {
            marginTop: "12px",
            padding: "12px",
            backgroundColor: colors.muted,
            border: `1px solid ${colors.border}`,
            borderRadius: "8px",
        },
        toolCallHeader: {
            display: "flex",
            alignItems: "center",
            gap: "8px",
            marginBottom: "8px",
            fontSize: "13px",
            fontWeight: 500,
            color: colors.foreground,
        },
        toolCallBadge: {
            fontSize: "11px",
            padding: "2px 8px",
            borderRadius: "4px",
            backgroundColor: colors.background,
            border: `1px solid ${colors.border}`,
            color: colors.mutedForeground,
        },
        successBadge: {
            backgroundColor: colors.success,
            color: "#ffffff",
            border: "none",
        },
        errorBadge: {
            backgroundColor: colors.error,
            color: "#ffffff",
            border: "none",
        },
        codeBlock: {
            fontSize: "12px",
            fontFamily: 'Consolas, Monaco, "Courier New", monospace',
            padding: "8px",
            backgroundColor: colors.background,
            borderRadius: "6px",
            whiteSpace: "pre-wrap" as const,
            wordBreak: "break-word" as const,
            overflow: "auto",
            maxHeight: "200px",
            margin: 0,
            color: colors.foreground,
        },
        imageContainer: {
            marginTop: "12px",
        },
        image: {
            maxWidth: "100%",
            maxHeight: "300px",
            objectFit: "contain" as const,
            borderRadius: "8px",
            border: `1px solid ${colors.border}`,
        },
    };

    const getMessageLabel = (messageType: string) => {
        const labels: Record<string, string> = {
            system: "系统提示",
            user: "用户",
            assistant: "助手",
            reasoning: "推理过程",
            response: "回复",
            error: "错误",
        };
        return labels[messageType] || messageType;
    };

    const isUserMessage = (type: string) => type === "user";
    const isSystemMessage = (type: string) => type === "system";
    const isReasoningMessage = (type: string) => type === "reasoning";

    const getBubbleStyle = (messageType: string) => {
        if (isUserMessage(messageType)) {
            return { ...styles.messageBubble, ...styles.userBubble };
        }
        if (isSystemMessage(messageType)) {
            return { ...styles.messageBubble, ...styles.systemBubble };
        }
        if (isReasoningMessage(messageType)) {
            return { ...styles.messageBubble, ...styles.reasoningBubble };
        }
        return { ...styles.messageBubble, ...styles.assistantBubble };
    };

    return (
        <div id="conversation-export-container" style={styles.container}>
            {/* 标题区域 */}
            <div style={styles.header}>
                <h1 style={styles.title}>{conversationName}</h1>
                <p style={styles.subtitle}>
                    助手: {assistantName} | 创建时间: {formatDate(createdTime)}
                </p>
            </div>

            {/* 消息列表 */}
            {filteredMessages.map((message, index) => {
                const isUser = isUserMessage(message.message_type);
                const isSystem = isSystemMessage(message.message_type);

                return (
                    <div
                        key={message.id || index}
                        style={{
                            ...styles.messageWrapper,
                            ...(isUser ? styles.userMessageWrapper : styles.assistantMessageWrapper),
                            ...(isSystem ? { alignItems: "stretch" as const } : {}),
                        }}
                    >
                        {/* 消息标签 */}
                        <div style={{
                            ...styles.messageLabel,
                            textAlign: isUser ? "right" : "left",
                            marginBottom: "10px"
                        }}>
                            {getMessageLabel(message.message_type)}
                        </div>

                        {/* 消息气泡 */}
                        <div style={getBubbleStyle(message.message_type)}>
                            {/* 消息内容 */}
                            <div style={styles.prose}>
                                <ExportMarkdown colors={colors}>{stripMcpToolCallMarkers(message.content || "")}</ExportMarkdown>
                            </div>

                            {/* 图片附件 */}
                            {message.attachment_list && message.attachment_list.length > 0 && (() => {
                                const imageAttachments = message.attachment_list.filter(
                                    (att: any) => att.attachment_type === "Image"
                                );
                                if (imageAttachments.length === 0) return null;

                                return (
                                    <div style={styles.imageContainer}>
                                        {imageAttachments.map((att: any, attIndex: number) => (
                                            <img
                                                key={attIndex}
                                                src={att.attachment_content || att.attachment_url}
                                                alt="Attachment"
                                                style={styles.image}
                                            />
                                        ))}
                                    </div>
                                );
                            })()}

                            {/* 工具调用参数 */}
                            {options.includeToolParams && (() => {
                                const nativeToolCalls = message.tool_calls_json
                                    ? parseToolCalls(message.tool_calls_json)
                                    : [];
                                const hintCalls = extractMcpToolCallHints(message.content || "");
                                if (nativeToolCalls.length === 0 && hintCalls.length === 0) return null;

                                return (
                                    <div style={{ marginTop: "12px" }}>
                                        {nativeToolCalls.map((tc, tcIndex) => {
                                            const parts = tc.fn_name.split("__");
                                            const toolName = parts.length > 1 ? parts.slice(1).join("__") : tc.fn_name;
                                            const serverName = parts[0] || "unknown";

                                            return (
                                                <div key={`native-${tcIndex}`} style={styles.toolCallBox}>
                                                    <div style={styles.toolCallHeader}>
                                                        <span>🔧</span>
                                                        <span>{serverName}</span>
                                                        <span style={{ color: colors.mutedForeground }}>-</span>
                                                        <span>{toolName}</span>
                                                        <span style={styles.toolCallBadge}>参数</span>
                                                    </div>
                                                    <pre style={styles.codeBlock}>
                                                        {JSON.stringify(tc.fn_arguments, null, 2)}
                                                    </pre>
                                                </div>
                                            );
                                        })}
                                        {hintCalls.map((hint, hintIndex) => {
                                            const fromDb = hint.call_id ? toolCallById.get(hint.call_id) : undefined;
                                            const serverName = fromDb?.server_name ?? hint.server_name ?? "unknown";
                                            const toolName = fromDb?.tool_name ?? hint.tool_name ?? "unknown";
                                            const paramsText = fromDb?.parameters ?? hint.parameters ?? "{}";
                                            return (
                                                <div key={`hint-${hintIndex}`} style={styles.toolCallBox}>
                                                    <div style={styles.toolCallHeader}>
                                                        <span>🔧</span>
                                                        <span>{serverName}</span>
                                                        <span style={{ color: colors.mutedForeground }}>-</span>
                                                        <span>{toolName}</span>
                                                        <span style={styles.toolCallBadge}>参数</span>
                                                    </div>
                                                    <pre style={styles.codeBlock}>
                                                        {formatJsonContent(paramsText)}
                                                    </pre>
                                                </div>
                                            );
                                        })}
                                    </div>
                                );
                            })()}

                            {/* 工具执行结果 */}
                            {options.includeToolResults && (() => {
                                const relatedById = new Map<number, (typeof toolCalls)[number]>();
                                const mappedCalls = toolCallMap.get(message.id) || [];
                                for (const tc of mappedCalls) {
                                    relatedById.set(tc.id, tc);
                                }
                                const hintCalls = extractMcpToolCallHints(message.content || "");
                                for (const hint of hintCalls) {
                                    if (!hint.call_id) continue;
                                    const call = toolCallById.get(hint.call_id);
                                    if (call) {
                                        relatedById.set(call.id, call);
                                    }
                                }
                                const relatedCalls = Array.from(relatedById.values());
                                if (relatedCalls.length === 0) return null;

                                return (
                                    <div style={{ marginTop: "12px" }}>
                                        {relatedCalls.map((tc, tcIndex) => (
                                            <div key={tcIndex} style={styles.toolCallBox}>
                                                <div style={styles.toolCallHeader}>
                                                    <span>🔧</span>
                                                    <span>{tc.server_name}</span>
                                                    <span style={{ color: colors.mutedForeground }}>-</span>
                                                    <span>{tc.tool_name}</span>
                                                    <span style={{
                                                        ...styles.toolCallBadge,
                                                        ...(tc.status === "success" ? styles.successBadge : {}),
                                                        ...(tc.status === "failed" ? styles.errorBadge : {}),
                                                    }}>
                                                        {tc.status === "success" ? "成功" : tc.status === "failed" ? "失败" : "执行中"}
                                                    </span>
                                                </div>
                                                {tc.status === "success" && tc.result && (
                                                    <pre style={styles.codeBlock}>
                                                        {formatJsonContent(tc.result)}
                                                    </pre>
                                                )}
                                                {tc.status === "failed" && tc.error && (
                                                    <div style={{ color: colors.error, fontSize: "13px", marginTop: "8px" }}>
                                                        错误: {tc.error}
                                                    </div>
                                                )}
                                            </div>
                                        ))}
                                    </div>
                                );
                            })()}
                        </div>
                    </div>
                );
            })}
        </div>
    );
};

/**
 * 渲染导出内容到指定的 DOM 容器（用于图片导出）
 */
export function renderExportContent(
    container: HTMLElement,
    data: ExportData,
    options: ConversationExportOptions,
): void {
    // 检测当前是否为暗色模式
    const isDarkMode = document.documentElement.classList.contains("dark");

    const root = createRoot(container);
    root.render(
        <ConversationExportRenderer
            data={data}
            options={options}
            conversationName={data.conversation.conversation.name}
            assistantName={data.conversation.conversation.assistant_name}
            createdTime={new Date(data.conversation.conversation.created_time)}
            isDarkMode={isDarkMode}
        />
    );
}

/**
 * 生成 PDF 导出的 HTML 内容（简洁文档样式，无气泡）
 * 适合打印和阅读的纯文档格式
 */
export function renderPdfExportContent(
    container: HTMLElement,
    data: ExportData,
    options: ConversationExportOptions,
): void {
    // PDF 始终使用浅色主题，更适合打印
    const { conversation, toolCalls } = data;
    const { messages } = conversation;

    // 构建工具调用映射
    const toolCallMap = mapToolCallsToMessages(toolCalls);

    // 过滤消息
    const filteredMessages = messages.filter((msg) => {
        if (msg.message_type === "tool_result") return false;
        if (msg.message_type === "user" && msg.content?.startsWith("Tool execution results:\n")) return false;
        if (msg.message_type === "system") return options.includeSystemPrompt;
        if (msg.message_type === "reasoning") return options.includeReasoning;
        return true;
    });

    const formatDate = (date: Date) => {
        return new Date(date).toLocaleString("zh-CN", {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
        });
    };

    const getMessageLabel = (messageType: string) => {
        const labels: Record<string, string> = {
            system: "系统提示",
            user: "用户",
            assistant: "助手",
            reasoning: "推理过程",
            response: "回复",
            error: "错误",
        };
        return labels[messageType] || messageType;
    };

    const escapeHtml = (str: string) => {
        return str
            .replace(/&/g, "&amp;")
            .replace(/</g, "&lt;")
            .replace(/>/g, "&gt;")
            .replace(/"/g, "&quot;")
            .replace(/'/g, "&#39;");
    };

    // 简单的 Markdown 转 HTML（保留基本格式）
    const markdownToHtml = (md: string) => {
        let html = escapeHtml(md);
        
        // 代码块 ```
        html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, _lang, code) => {
            return `<pre style="background: #f5f5f5; padding: 10px; border-radius: 4px; margin: 8px 0; border: 1px solid #e0e0e0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 11px; color: #333; white-space: pre-wrap; word-break: break-word;">${code}</code></pre>`;
        });
        
        // 行内代码
        html = html.replace(/`([^`]+)`/g, '<code style="background: #f5f5f5; padding: 1px 4px; border-radius: 3px; font-family: Consolas, Monaco, monospace; font-size: 0.9em; color: #333;">$1</code>');
        
        // 标题
        html = html.replace(/^### (.*$)/gm, '<h4 style="font-size: 13px; font-weight: 600; margin: 10px 0 5px; color: #111;">$1</h4>');
        html = html.replace(/^## (.*$)/gm, '<h3 style="font-size: 14px; font-weight: 600; margin: 10px 0 5px; color: #111;">$1</h3>');
        html = html.replace(/^# (.*$)/gm, '<h2 style="font-size: 15px; font-weight: 600; margin: 10px 0 5px; color: #111;">$1</h2>');
        
        // 粗体和斜体
        html = html.replace(/\*\*\*(.+?)\*\*\*/g, '<strong><em>$1</em></strong>');
        html = html.replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>');
        html = html.replace(/\*(.+?)\*/g, '<em>$1</em>');
        
        // 无序列表
        html = html.replace(/^\s*[-*+]\s+(.*)$/gm, '<li style="margin: 2px 0;">$1</li>');
        html = html.replace(/(<li[^>]*>.*<\/li>\n?)+/g, '<ul style="margin: 5px 0; padding-left: 18px;">$&</ul>');
        
        // 有序列表
        html = html.replace(/^\s*\d+\.\s+(.*)$/gm, '<li style="margin: 2px 0;">$1</li>');

        // 图片
        html = html.replace(
            /!\[([^\]]*)\]\(([^)]+)\)/g,
            '<img src="$2" alt="$1" style="max-width: 100%; height: auto; border: 1px solid #e5e5e5; border-radius: 6px; margin: 8px 0;" />',
        );
        
        // 链接
        html = html.replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" style="color: #2563eb; text-decoration: underline;">$1</a>');
        
        // 换行
        html = html.replace(/\n\n/g, '</p><p style="margin: 5px 0;">');
        html = html.replace(/\n/g, '<br>');
        
        return `<p style="margin: 5px 0;">${html}</p>`;
    };

    // 生成消息 HTML（简洁文档样式，无气泡）
    const generateMessagesHtml = () => {
        return filteredMessages.map((message) => {
            const label = getMessageLabel(message.message_type);

            let toolCallsHtml = "";
            let imageAttachmentsHtml = "";
            
            // 工具调用参数
            if (options.includeToolParams) {
                const parsedCalls = message.tool_calls_json ? parseToolCalls(message.tool_calls_json) : [];
                if (parsedCalls.length > 0) {
                    toolCallsHtml += parsedCalls.map((tc) => {
                        const parts = tc.fn_name.split("__");
                        const toolName = parts.length > 1 ? parts.slice(1).join("__") : tc.fn_name;
                        const serverName = parts[0] || "unknown";
                        return `
                            <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid #2563eb; font-size: 11px;">
                                <div style="font-weight: 500; margin-bottom: 4px; color: #333;">🔧 ${escapeHtml(serverName)} / ${escapeHtml(toolName)}</div>
                                <pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${escapeHtml(JSON.stringify(tc.fn_arguments, null, 2))}</code></pre>
                            </div>
                        `;
                    }).join("");
                } else if (toolCallMap.has(message.id)) {
                    const mappedCalls = toolCallMap.get(message.id) || [];
                    toolCallsHtml += mappedCalls.map((tc) => {
                        return `
                            <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid #2563eb; font-size: 11px;">
                                <div style="font-weight: 500; margin-bottom: 4px; color: #333;">🔧 ${escapeHtml(tc.server_name)} / ${escapeHtml(tc.tool_name)}</div>
                                <pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${escapeHtml(formatJsonContent(tc.parameters))}</code></pre>
                            </div>
                        `;
                    }).join("");
                }
            }

            // 工具执行结果
            if (options.includeToolResults && toolCallMap.has(message.id)) {
                const relatedCalls = (toolCallMap.get(message.id) || []).filter(
                    (call) => call.status === "success" && call.result,
                );
                if (relatedCalls.length > 0) {
                    toolCallsHtml += relatedCalls.map((tc) => {
                        const statusText = tc.status === "success" ? "✓" : tc.status === "failed" ? "✗" : "...";
                        let resultHtml = "";
                        if (tc.result) {
                            resultHtml = `<pre style="background: #f0f0f0; padding: 6px; border-radius: 3px; margin: 4px 0 0; overflow-x: auto;"><code style="font-family: Consolas, Monaco, monospace; font-size: 10px; color: #333; white-space: pre-wrap; word-break: break-word;">${escapeHtml(formatJsonContent(tc.result))}</code></pre>`;
                        }
                        return `
                            <div style="margin-top: 8px; padding: 8px; background: #f9f9f9; border-left: 3px solid ${tc.status === "success" ? "#22c55e" : tc.status === "failed" ? "#dc2626" : "#666"}; font-size: 11px;">
                                <div style="font-weight: 500; margin-bottom: 4px; color: #333;">${statusText} ${escapeHtml(tc.server_name)} / ${escapeHtml(tc.tool_name)}</div>
                                ${resultHtml}
                            </div>
                        `;
                    }).join("");
                }
            }

            if (Array.isArray(message.attachment_list)) {
                const imageAttachments = message.attachment_list.filter(
                    (att: any) => att?.attachment_type === "Image",
                );
                if (imageAttachments.length > 0) {
                    imageAttachmentsHtml = imageAttachments
                        .map((att: any) => {
                            const imageSrc = att?.attachment_content || att?.attachment_url;
                            if (!imageSrc) return "";
                            const imageAlt =
                                att?.attachment_url?.split(/[\\/]/).pop() || "attachment-image";
                            return `
                                <div style="margin-top: 8px;">
                                    <img src="${escapeHtml(String(imageSrc))}" alt="${escapeHtml(String(imageAlt))}" style="max-width: 100%; height: auto; border: 1px solid #e5e5e5; border-radius: 6px;" />
                                </div>
                            `;
                        })
                        .join("");
                }
            }

            return `
                <div style="margin-bottom: 16px; padding-bottom: 12px; border-bottom: 1px solid #e5e5e5; page-break-inside: avoid; break-inside: avoid;">
                    <div style="font-size: 11px; font-weight: 600; color: #666; margin-bottom: 6px;">${escapeHtml(label)}</div>
                    <div style="color: #111; font-size: 12px; line-height: 1.6;">${markdownToHtml(stripMcpToolCallMarkers(message.content || ""))}</div>
                    ${imageAttachmentsHtml}
                    ${toolCallsHtml}
                </div>
            `;
        }).join("");
    };

    // 设置容器样式并填充内容
    container.style.fontFamily = '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, "Microsoft YaHei", sans-serif';
    container.style.lineHeight = "1.5";
    container.style.color = "#111";
    container.style.background = "#ffffff";
    container.style.fontSize = "12px";
    container.style.padding = "24px";

    container.innerHTML = `
        <div style="margin-bottom: 20px; padding-bottom: 12px; border-bottom: 2px solid #111;">
            <h1 style="font-size: 18px; font-weight: 600; margin: 0 0 6px 0; color: #111;">${escapeHtml(data.conversation.conversation.name)}</h1>
            <p style="font-size: 11px; color: #666; margin: 0;">
                助手: ${escapeHtml(data.conversation.conversation.assistant_name)} | 
                创建时间: ${formatDate(new Date(data.conversation.conversation.created_time))}
            </p>
        </div>
        ${generateMessagesHtml()}
    `;
}

export default ConversationExportRenderer;
