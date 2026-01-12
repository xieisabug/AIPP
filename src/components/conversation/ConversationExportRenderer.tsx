import React from "react";
import { createRoot } from "react-dom/client";
import UnifiedMarkdown from "../UnifiedMarkdown";
import { Badge } from "../ui/badge";
import { Blocks } from "lucide-react";
import type { ExportData, ConversationExportOptions } from "@/utils/exportFormatters";
import { parseToolCalls, mapToolCallsToMessages } from "@/utils/exportFormatters";

interface ConversationExportRendererProps {
    data: ExportData;
    options: ConversationExportOptions;
    conversationName: string;
    assistantName: string;
    createdTime: Date;
}

/**
 * 对话导出渲染器 - 用于 PDF/图片导出
 * 使用与页面相同的组件来渲染，确保样式一致
 */
const ConversationExportRenderer: React.FC<ConversationExportRendererProps> = ({
    data,
    options,
    conversationName,
    assistantName,
    createdTime,
}) => {
    const { conversation, toolCalls } = data;
    const { messages } = conversation;

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

    return (
        <div
            id="conversation-export-container"
            className="w-full bg-background text-foreground"
            style={{
                padding: "20px",
                fontFamily:
                    '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Helvetica Neue", Arial, sans-serif',
                lineHeight: "1.6",
            }}
        >
            {/* 标题 */}
            <h1 className="text-2xl font-semibold mb-2">{conversationName}</h1>
            <div className="text-sm text-muted-foreground mb-6">
                助手: {assistantName} | 创建时间: {formatDate(createdTime)}
            </div>
            <hr className="border-border mb-6" />

            {/* 消息列表 */}
            {filteredMessages.map((message, index) => (
                <div key={message.id || index} className="mb-6">
                    {/* 消息标题 */}
                    <div className="text-sm font-medium text-muted-foreground mb-2">
                        {getMessageLabel(message.message_type)}
                    </div>

                    {/* 消息内容 */}
                    <div className="text-foreground">
                        <UnifiedMarkdown>{message.content || ""}</UnifiedMarkdown>

                        {/* 图片附件 */}
                        {message.attachment_list && message.attachment_list.length > 0 && (() => {
                            const imageAttachments = message.attachment_list.filter(
                                (att: any) => att.attachment_type === "Image"
                            );
                            if (imageAttachments.length === 0) return null;

                            return (
                                <div className="mt-3 flex flex-col gap-2">
                                    {imageAttachments.map((att: any, attIndex: number) => (
                                        <img
                                            key={attIndex}
                                            src={att.attachment_content || att.attachment_url}
                                            alt="Attachment"
                                            className="max-w-full h-auto rounded-md border border-border"
                                            style={{ maxHeight: "400px", objectFit: "contain" }}
                                        />
                                    ))}
                                </div>
                            );
                        })()}
                    </div>

                    {/* 工具调用参数 */}
                    {options.includeToolParams && message.tool_calls_json && (() => {
                        const toolCalls = parseToolCalls(message.tool_calls_json);
                        if (toolCalls.length === 0) return null;

                        return (
                            <div className="mt-3 space-y-2">
                                {toolCalls.map((tc, tcIndex) => {
                                    const parts = tc.fn_name.split("__");
                                    const toolName =
                                        parts.length > 1 ? parts.slice(1).join("__") : tc.fn_name;
                                    const serverName = parts[0] || "unknown";

                                    return (
                                        <div
                                            key={tcIndex}
                                            className="w-full p-2 border border-border rounded-md bg-card"
                                        >
                                            <div className="flex items-center justify-between mb-2">
                                                <div className="flex items-center gap-2 text-sm">
                                                    <Blocks className="h-4 w-4" />
                                                    <span className="truncate">{serverName}</span>
                                                    <span className="text-xs font-bold text-muted-foreground">
                                                        {" "}
                                                        -{" "}
                                                    </span>
                                                    <span className="truncate">{toolName}</span>
                                                </div>
                                                <Badge variant="outline" className="ml-3">
                                                    参数
                                                </Badge>
                                            </div>
                                            <pre className="text-xs font-mono p-2 whitespace-pre-wrap break-words mt-0 mb-0 bg-muted text-foreground rounded-md overflow-auto max-h-48">
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
                            <div className="mt-3 space-y-2">
                                {relatedCalls.map((tc, tcIndex) => (
                                    <div
                                        key={tcIndex}
                                        className="w-full p-2 border border-border rounded-md bg-card"
                                    >
                                        <div className="flex items-center justify-between mb-2">
                                            <div className="flex items-center gap-2 text-sm">
                                                <Blocks className="h-4 w-4" />
                                                <span className="truncate">{tc.server_name}</span>
                                                <span className="text-xs font-bold text-muted-foreground">
                                                    {" "}
                                                    -{" "}
                                                </span>
                                                <span className="truncate">{tc.tool_name}</span>
                                            </div>
                                            <Badge
                                                variant={
                                                    tc.status === "success"
                                                        ? "default"
                                                        : tc.status === "failed"
                                                        ? "destructive"
                                                        : "secondary"
                                                }
                                                className="ml-3"
                                            >
                                                {tc.status === "success"
                                                    ? "成功"
                                                    : tc.status === "failed"
                                                    ? "失败"
                                                    : "执行中"}
                                            </Badge>
                                        </div>
                                        {tc.status === "success" && tc.result && (
                                            <pre className="text-xs font-mono p-2 whitespace-pre-wrap break-words mt-0 mb-0 bg-muted text-foreground rounded-md overflow-auto max-h-48">
                                                {tc.result}
                                            </pre>
                                        )}
                                        {tc.status === "failed" && tc.error && (
                                            <div className="text-destructive text-sm mt-1">
                                                错误: {tc.error}
                                            </div>
                                        )}
                                    </div>
                                ))}
                            </div>
                        );
                    })()}

                    <hr className="border-border mt-4" />
                </div>
            ))}
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
    const root = createRoot(container);
    root.render(
        <ConversationExportRenderer
            data={data}
            options={options}
            conversationName={data.conversation.conversation.name}
            assistantName={data.conversation.conversation.assistant_name}
            createdTime={new Date(data.conversation.conversation.created_time)}
        />
    );
}

export default ConversationExportRenderer;
