import React, { useState } from "react";
import IconButton from "../IconButton";
import { Copy, Check } from "lucide-react";
import { useCopyHandler } from "@/hooks/useCopyHandler";
import { ChevronDown, ChevronRight } from "lucide-react";

interface ErrorMessageProps {
    content: string;
    messageId?: number;
}

interface ParsedErrorContent {
    mainMessage: string;
    details: string | null;
    hasDetails: boolean;
    meta?: Record<string, any> | null;
}

// HTTP 状态码描述映射
const getStatusDescription = (status: number): string => {
    const statusMap: Record<number, string> = {
        400: "Bad Request - 请求参数错误",
        401: "Unauthorized - 认证失败",
        403: "Forbidden - 权限不足",
        404: "Not Found - 资源不存在",
        429: "Too Many Requests - 请求过于频繁",
        500: "Internal Server Error - 服务器内部错误",
        502: "Bad Gateway - 网关错误",
        503: "Service Unavailable - 服务不可用",
        504: "Gateway Timeout - 网关超时",
    };
    return statusMap[status] || `HTTP ${status}`;
};

// 状态码对应的样式
const getStatusBadgeStyle = (status: number): string => {
    if (status >= 500) {
        return "bg-red-200 text-red-800 border-red-300";
    } else if (status >= 400) {
        return "bg-orange-200 text-orange-800 border-orange-300";
    }
    return "bg-gray-200 text-gray-800 border-gray-300";
};

const ErrorMessage: React.FC<ErrorMessageProps> = ({ content, messageId }) => {
    const [isExpanded, setIsExpanded] = useState(false);
    const { copyIconState, handleCopy } = useCopyHandler(content);

    // Parse error content to extract main message and details
    const parseErrorContent = (content: string): ParsedErrorContent => {
        // 首先检查是否使用了新的分隔符格式
        const delimiter = "|||ERROR_DETAILS|||";
        if (content.includes(delimiter)) {
            const parts = content.split(delimiter);
            if (parts.length === 2) {
                return {
                    mainMessage: parts[0],
                    details: parts[1],
                    hasDetails: true,
                    meta: null,
                };
            }
        }

        // 兼容旧的JSON格式
        try {
            const parsed = JSON.parse(content);
            if (parsed.message && (parsed.details !== undefined)) {
                return {
                    mainMessage: parsed.message,
                    details: typeof parsed.details === "string" ? parsed.details : JSON.stringify(parsed.details),
                    hasDetails: !!parsed.details,
                    meta: parsed,
                };
            }
        } catch (e) {
            // Not JSON, try to extract details from text
        }

        // Look for patterns that indicate request body information (向后兼容)
        const detailsPatterns = [
            /\[\[extracted_error_body\]\]: (.+)/,
            /\[\[error_response_body\]\]: (.+)/,
            /\[\[empty_post_error_body\]\]: (.+)/,
            /Request body: (.+)/i,
            /Response: (.+)/i,
        ];

        for (const pattern of detailsPatterns) {
            const match = content.match(pattern);
            if (match) {
                const details = match[1];
                const mainMessage = content.replace(pattern, "").trim();

                // Check if details look like JSON or HTML
                const isStructuredDetails =
                    details.startsWith("{") ||
                    details.startsWith("<") ||
                    details.length > 100;

                return {
                    mainMessage: mainMessage || "请求失败",
                    details: details,
                    hasDetails: isStructuredDetails,
                    meta: null,
                };
            }
        }

        // If content is very long, consider it might have embedded details
        if (content.length > 200) {
            const lines = content.split("\n");
            if (lines.length > 3) {
                return {
                    mainMessage: lines[0],
                    details: lines.slice(1).join("\n"),
                    hasDetails: true,
                    meta: null,
                };
            }
        }

        return {
            mainMessage: content,
            details: null,
            hasDetails: false,
            meta: null,
        };
    };

    const { mainMessage, details, hasDetails, meta } = parseErrorContent(content);

    const formatDetails = (details: string) => {
        try {
            // Try to format as JSON if it's valid JSON
            const parsed = JSON.parse(details);
            return JSON.stringify(parsed, null, 2);
        } catch (e) {
            // Return as-is if not JSON
            return details;
        }
    };

    return (
        <div
            data-message-item
            data-message-id={messageId}
            data-message-type="error"
            className="group relative w-fit max-w-[min(65%,56rem)] overflow-hidden rounded-2xl border border-red-200 bg-red-50 px-5 py-4 text-red-800 transition-all duration-200 self-start"
        >
            <div className="flex items-start gap-3">
                <div className="flex-shrink-0 w-5 h-5 mt-0.5">
                    <svg
                        className="w-5 h-5 text-red-500"
                        fill="currentColor"
                        viewBox="0 0 20 20"
                    >
                        <path
                            fillRule="evenodd"
                            d="M18 10a8 8 0 11-16 0 8 8 0 0116 0zm-7 4a1 1 0 11-2 0 1 1 0 012 0zm-1-9a1 1 0 00-1 1v4a1 1 0 102 0V6a1 1 0 00-1-1z"
                            clipRule="evenodd"
                        />
                    </svg>
                </div>
                <div className="min-w-0 flex-1">
                    <div className="text-sm font-medium text-red-800 mb-1">
                        AI Request Failed
                    </div>
                    <div className="prose prose-sm max-w-none break-words text-red-700">
                        {mainMessage}
                    </div>
                    {meta && (
                        <div className="mt-2 text-xs text-red-700 space-y-2">
                            {/* HTTP 状态码 - 突出显示 */}
                            {meta.status && (
                                <div className="flex flex-wrap items-center gap-2">
                                    <span className={`inline-flex items-center px-2 py-0.5 rounded text-xs font-mono font-bold border ${getStatusBadgeStyle(meta.status)}`}>
                                        {meta.status}
                                    </span>
                                    <span className="break-words text-red-600/80">
                                        {getStatusDescription(meta.status)}
                                    </span>
                                </div>
                            )}

                            {/* 基本信息网格 */}
                            <div className="grid grid-cols-1 gap-x-4 gap-y-1 sm:grid-cols-2">
                                {meta.model && (
                                    <div className="min-w-0">
                                        <span className="text-red-600/80">模型：</span>
                                        <span className="font-medium break-all">{meta.model}</span>
                                    </div>
                                )}
                                {meta.phase && (
                                    <div className="min-w-0">
                                        <span className="text-red-600/80">阶段：</span>
                                        <span className="font-medium break-words">{meta.phase === 'stream' ? '流式请求' : meta.phase === 'non_stream' ? '非流式请求' : meta.phase}</span>
                                    </div>
                                )}
                                {typeof meta.attempts !== "undefined" && meta.attempts !== null && (
                                    <div className="min-w-0">
                                        <span className="text-red-600/80">重试次数：</span>
                                        <span className="font-medium">{meta.attempts}</span>
                                    </div>
                                )}
                                {meta.request_id && (
                                    <div className="min-w-0 break-all sm:col-span-2">
                                        <span className="text-red-600/80">请求ID：</span>
                                        <span className="font-medium font-mono text-[11px]">{String(meta.request_id)}</span>
                                    </div>
                                )}
                                {meta.endpoint && (
                                    <div className="min-w-0 break-all sm:col-span-2">
                                        <span className="text-red-600/80">端点：</span>
                                        <span className="font-medium font-mono text-[11px]">{String(meta.endpoint)}</span>
                                    </div>
                                )}
                            </div>

                            {/* 建议标签 */}
                            {Array.isArray(meta.suggestions) && meta.suggestions.length > 0 && (
                                <div className="flex flex-wrap gap-1 pt-1">
                                    <span className="text-red-600/80 text-[11px]">建议：</span>
                                    {meta.suggestions.map((s: string, idx: number) => (
                                        <span
                                            key={idx}
                                            className="inline-flex items-center px-2 py-0.5 rounded bg-red-100 text-red-700 border border-red-200 text-[11px]"
                                        >
                                            {s}
                                        </span>
                                    ))}
                                </div>
                            )}
                        </div>
                    )}
                    {hasDetails && (
                        <div className="mt-3">
                            <button
                                onClick={() => setIsExpanded(!isExpanded)}
                                className="flex items-center gap-1 text-xs text-red-600 transition-colors hover:text-red-800"
                            >
                                {isExpanded ? (
                                    <ChevronDown className="w-3 h-3 text-icon" />
                                ) : (
                                    <ChevronRight className="w-3 h-3 text-icon" />
                                )}
                                <span>
                                    {isExpanded ? "隐藏详情" : "查看详情（响应体/原始错误）"}
                                </span>
                            </button>
                            {isExpanded && (
                                <div className="mt-2 w-full max-w-full space-y-3 rounded-lg border border-red-200 bg-red-100 p-3">
                                    {/* 响应体 / 错误详情 - 重点展示 */}
                                    {details && (
                                        <div>
                                            <div className="flex items-center justify-between mb-1">
                                                <div className="text-[11px] text-red-600/80 font-medium">
                                                    {meta?.status ? `HTTP ${meta.status} 响应体` : '错误详情'}
                                                </div>
                                            </div>
                                            <div className="max-w-full overflow-hidden rounded border border-red-200 bg-white/50 p-2">
                                                <pre className="max-w-full overflow-auto whitespace-pre-wrap break-words text-xs font-mono text-red-800">
                                                    {formatDetails(details)}
                                                </pre>
                                            </div>
                                        </div>
                                    )}

                                    {/* 原始错误信息 */}
                                    {meta && meta.original_error && (
                                        <div>
                                            <div className="text-[11px] text-red-600/80 mb-1 font-medium">原始错误信息</div>
                                            <div className="max-w-full overflow-hidden rounded border border-red-200 bg-white/30 p-2">
                                                <pre className="max-h-32 max-w-full overflow-auto whitespace-pre-wrap break-words text-[11px] font-mono text-red-700">
                                                    {String(meta.original_error)}
                                                </pre>
                                            </div>
                                        </div>
                                    )}
                                </div>
                            )}
                        </div>
                    )}
                </div>
            </div>
            <div className="hidden group-hover:flex items-center absolute -bottom-9 py-3 px-4 box-border h-10 rounded-[21px] border border-red-200 bg-red-50 left-0">
                <IconButton
                    icon={
                        copyIconState === "copy" ? (
                            <Copy size={16} className="text-icon" />
                        ) : (
                            <Check size={16} className="text-icon" />
                        )
                    }
                    onClick={handleCopy}
                />
            </div>
        </div>
    );
};

export default ErrorMessage;
