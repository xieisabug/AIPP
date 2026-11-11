import React, { useState } from "react";
import { Copy, Check } from "lucide-react";
import { useCopyHandler } from "@/hooks/useCopyHandler";
import { ChevronDown, ChevronRight, AlertCircle } from "lucide-react";
import { useIsMobilePlatform } from "@/hooks/use-platform";

interface ErrorMessageProps {
    content: string;
    messageId?: number;
    superseded?: boolean;
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


/** 从 genai 的多行错误里取根因："Cause: xxx" 优先，否则取最后一行。 */
export function extractRootCause(originalError: string): string {
    const lines = originalError.split("\n").map((line) => line.trim()).filter(Boolean);
    const causeLine = lines.find((line) => /^cause:/i.test(line));
    const cause = causeLine ? causeLine.replace(/^cause:\s*/i, "") : lines[lines.length - 1] ?? "";
    return cause.length > 300 ? `${cause.slice(0, 300)}…` : cause;
}

/** 响应体里服务端自己给出的错误说明（OpenAI 兼容格式 `error.message` 或顶层 `message`）。 */
function extractDetailsMessage(details: string): string | null {
    try {
        const parsed = JSON.parse(details);
        const message = typeof parsed?.error === "string" ? parsed.error : parsed?.error?.message ?? parsed?.message;
        return typeof message === "string" && message.trim() ? message.trim() : null;
    } catch {
        return null;
    }
}

/** 折叠行用的一句话：概括文案 + 具体原因（根因或服务端错误说明），两者重复时只留一个。 */
export function summarizeError(mainMessage: string, details: string | null, rootCause: string | null): {
    summary: string;
    reason: string | null;
} {
    const summary = mainMessage.replace(/^AI\s*请求失败\s*[:：]\s*/, "").trim() || mainMessage;
    const rawReason = rootCause || (details ? extractDetailsMessage(details) : null);
    const reason = rawReason && !summary.includes(rawReason)
        ? (rawReason.length > 200 ? `${rawReason.slice(0, 200)}…` : rawReason)
        : null;
    return { summary, reason };
}

const ErrorMessage: React.FC<ErrorMessageProps> = ({ content, messageId, superseded = false }) => {
    // 默认折叠成一行，点开才显示完整错误卡片（ChatUI 与管家共用）。
    const [isOpen, setIsOpen] = useState(false);
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
                    details: parsed.details == null
                        ? null
                        : typeof parsed.details === "string" ? parsed.details : JSON.stringify(parsed.details),
                    // 没有响应体时（如网络错误）也要能看到原始错误
                    hasDetails: !!parsed.details || !!parsed.original_error,
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
    const isMobilePlatform = useIsMobilePlatform();
    // 无响应体时，直接展示原始错误中的根因（如 "error sending request for url ..."）
    const rootCause = !details && meta?.original_error ? extractRootCause(String(meta.original_error)) : null;

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

    const { summary, reason } = summarizeError(mainMessage, details, rootCause);
    const headline = superseded ? "此前请求失败" : `回复失败：${summary}${reason ? `（${reason}）` : ""}`;
    const action = isOpen ? "收起" : superseded ? "查看错误" : "查看详情";
    const label = `${headline} · ${action}`;
    const phaseLabel = meta?.phase === "stream" ? "流式请求" : meta?.phase === "non_stream" ? "非流式请求" : meta?.phase;
    const facts: Array<[string, React.ReactNode]> = [];
    if (meta?.status) facts.push(["状态", `${meta.status} · ${getStatusDescription(meta.status)}`]);
    if (meta?.model) facts.push(["模型", meta.model]);
    if (phaseLabel) facts.push(["阶段", phaseLabel]);
    if (meta?.attempts !== undefined && meta?.attempts !== null) facts.push(["重试次数", meta.attempts]);
    if (meta?.request_id) facts.push(["请求ID", <span className="font-mono">{String(meta.request_id)}</span>]);
    if (meta?.endpoint) facts.push(["端点", <span className="font-mono">{String(meta.endpoint)}</span>]);
    const suggestions: string[] = Array.isArray(meta?.suggestions) ? meta.suggestions : [];

    // 折叠和展开共用同一行标题：展开只是在下方补充详情，不换成另一套卡片样式。
    return (
        <div data-message-item data-message-id={messageId} data-message-type="error" className="flex w-full min-w-0 flex-col items-start">
            <button type="button" aria-expanded={isOpen} aria-label={label} title={label} onClick={() => setIsOpen(!isOpen)}
                className={`flex min-h-11 min-w-0 max-w-full items-center gap-2 text-left text-sm transition-colors ${superseded ? "text-muted-foreground hover:text-foreground" : "text-foreground hover:text-foreground/80"}`}>
                <AlertCircle className={`h-4 w-4 shrink-0 ${superseded ? "" : "text-red-500 dark:text-red-400"}`} />
                {superseded ? <span>{headline}</span> : (
                    <span className={`min-w-0 ${isOpen ? "break-words" : "truncate"}`}>
                        回复失败：<span data-aipp-slot="error-summary">{summary}</span>
                        {reason && <span className="text-muted-foreground" data-aipp-slot="error-reason">（{reason}）</span>}
                    </span>
                )}
                <span className="shrink-0 text-muted-foreground">· {action}</span>
            </button>
            {isOpen && (
                <div data-aipp-slot="error-details"
                    className={`${isMobilePlatform ? "w-full max-w-full" : "max-w-[min(65%,56rem)]"} ml-2 min-w-0 space-y-2 border-l border-border pb-1 pl-4 text-xs text-muted-foreground`}>
                    {/* 标题行已写明概括与原因；此前的错误标题只有“此前请求失败”，在这里补上 */}
                    {superseded && (
                        <p className="break-words text-sm text-foreground" data-aipp-slot="error-main-message">
                            {summary}{reason && <span className="text-muted-foreground">（{reason}）</span>}
                        </p>
                    )}
                    {facts.length > 0 && (
                        <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1">
                            {facts.map(([name, value]) => (
                                <React.Fragment key={name}>
                                    <dt>{name}</dt>
                                    <dd className="break-all text-foreground">{value}</dd>
                                </React.Fragment>
                            ))}
                        </dl>
                    )}
                    {suggestions.length > 0 && (
                        <p className="break-words">建议：<span className="text-foreground">{suggestions.join("；")}</span></p>
                    )}
                    {hasDetails && (
                        <div>
                            <button type="button" onClick={() => setIsExpanded(!isExpanded)} aria-expanded={isExpanded}
                                className="flex min-h-8 items-center gap-1 transition-colors hover:text-foreground">
                                {isExpanded ? <ChevronDown className="h-3 w-3" /> : <ChevronRight className="h-3 w-3" />}
                                <span>{isExpanded ? "隐藏详情" : "查看详情（响应体/原始错误）"}</span>
                            </button>
                            {isExpanded && (
                                <div className="mt-1 space-y-2">
                                    {details && (
                                        <div>
                                            <div className="mb-1">{meta?.status ? `HTTP ${meta.status} 响应体` : "错误详情"}</div>
                                            <pre className="max-h-64 max-w-full overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-muted/50 p-2 font-mono text-[11px] text-foreground">
                                                {formatDetails(details)}
                                            </pre>
                                        </div>
                                    )}
                                    {meta?.original_error && (
                                        <div>
                                            <div className="mb-1">原始错误信息</div>
                                            <pre className="max-h-32 max-w-full overflow-auto whitespace-pre-wrap break-words rounded-md border border-border bg-muted/50 p-2 font-mono text-[11px] text-foreground">
                                                {String(meta.original_error)}
                                            </pre>
                                        </div>
                                    )}
                                </div>
                            )}
                        </div>
                    )}
                    <button type="button" onClick={handleCopy}
                        className={`flex ${isMobilePlatform ? "min-h-11" : "min-h-8"} items-center gap-1 transition-colors hover:text-foreground`}>
                        {copyIconState === "copy" ? <Copy className="h-3 w-3" /> : <Check className="h-3 w-3" />}
                        <span>{copyIconState === "copy" ? "复制错误" : "已复制"}</span>
                    </button>
                </div>
            )}
        </div>
    );
};

export default ErrorMessage;
