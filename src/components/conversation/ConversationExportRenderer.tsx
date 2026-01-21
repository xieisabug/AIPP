import React from "react";
import { createRoot } from "react-dom/client";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import type { ExportData, ConversationExportOptions } from "@/utils/exportFormatters";
import { parseToolCalls, mapToolCallsToMessages } from "@/utils/exportFormatters";

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
    code: ({ className, children, ...props }: any) => {
        const isBlock = className?.includes("language-");
        if (isBlock) {
            return (
                <pre style={{
                    backgroundColor: colors.codeBg,
                    padding: "12px",
                    borderRadius: "8px",
                    overflow: "auto",
                    margin: "0.75em 0",
                    border: `1px solid ${colors.border}`,
                }}>
                    <code style={{
                        fontFamily: 'Consolas, Monaco, "Courier New", monospace',
                        fontSize: "13px",
                        color: colors.codeText,
                        whiteSpace: "pre-wrap",
                        wordBreak: "break-word",
                    }} {...props}>{children}</code>
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
            remarkPlugins={[remarkGfm]}
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

    // 过滤消息
    const filteredMessages = messages.filter((msg) => {
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
                                <ExportMarkdown colors={colors}>{message.content || ""}</ExportMarkdown>
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
                            {options.includeToolParams && message.tool_calls_json && (() => {
                                const parsedToolCalls = parseToolCalls(message.tool_calls_json);
                                if (parsedToolCalls.length === 0) return null;

                                return (
                                    <div style={{ marginTop: "12px" }}>
                                        {parsedToolCalls.map((tc, tcIndex) => {
                                            const parts = tc.fn_name.split("__");
                                            const toolName = parts.length > 1 ? parts.slice(1).join("__") : tc.fn_name;
                                            const serverName = parts[0] || "unknown";

                                            return (
                                                <div key={tcIndex} style={styles.toolCallBox}>
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
                                    </div>
                                );
                            })()}

                            {/* 工具执行结果 */}
                            {options.includeToolResults && toolCallMap.has(message.id) && (() => {
                                const relatedCalls = toolCallMap.get(message.id);
                                if (!relatedCalls || relatedCalls.length === 0) return null;

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
                                                        {tc.result}
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
 * 渲染导出内容到指定的 DOM 容器
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

export default ConversationExportRenderer;
