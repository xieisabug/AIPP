import { useMemo, useState } from "react";
import { ChevronDown, ChevronRight, KeyRound, Loader2 } from "lucide-react";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";

interface VaultResult {
    credential?: string;
    method?: string;
    url?: string;
    status?: number;
    body?: string;
    truncated?: boolean;
}

interface VaultError {
    error?: { code?: string; message?: string; credential?: string };
}

const parseJson = (text: string | undefined): unknown => {
    if (!text) return undefined;
    try {
        return JSON.parse(text);
    } catch {
        return undefined;
    }
};

/** 工具结果是 MCP content 数组，真正的结果在第一段 text 里。 */
export function parseVaultToolResult(result: string | undefined): (VaultResult & VaultError) | undefined {
    const parsed = parseJson(result);
    const parts = Array.isArray(parsed) ? parsed : (parsed as { content?: unknown[] } | undefined)?.content;
    const text = Array.isArray(parts)
        ? parts.map((part) => (part && typeof part === "object" && "text" in part ? String((part as { text: unknown }).text) : "")).join("")
        : typeof result === "string" ? result : "";
    const inner = parseJson(text);
    return inner && typeof inner === "object" ? (inner as VaultResult & VaultError) : undefined;
}

const ERROR_HINTS: Record<string, string> = {
    user_denied: "你拒绝了这次使用",
    site_not_approved: "此网站还没批准（无人值守任务不能询问）",
    approval_timeout: "等待确认超时",
    credential_not_found: "保险箱里没有这条凭据",
    credential_ambiguous: "有重名的凭据，请在保险箱中改名",
    vault_locked: "保险箱暂时无法解密，请解锁系统钥匙串或输入同步口令",
    vault_unavailable_on_device: "此设备不支持保险箱",
    header_not_allowed: "请求里包含了由保险箱注入的认证头或参数",
    upstream_error: "目标服务请求失败",
    invalid_request: "请求参数无效",
};

export const VAULT_CARD_STATUSES = new Set(["executing", "success", "failed"]);

/** vault_http_request 的结果卡：一行显示访问了哪里，展开看响应。其他状态（如待执行）交给通用卡片。 */
export default function VaultRequestToolCall(props: McpToolComponentProps) {
    const [expanded, setExpanded] = useState(false);
    const call = props.currentToolCall;
    const status = call?.status ?? props.status;
    const params = useMemo(() => parseJson(props.parameters) as { credential?: string; method?: string; url?: string } | undefined, [props.parameters]);
    const parsed = useMemo(() => parseVaultToolResult(call?.result ?? call?.error), [call?.result, call?.error]);

    const method = parsed?.method ?? params?.method ?? "GET";
    const url = parsed?.url ?? params?.url ?? "";
    const credential = parsed?.credential ?? parsed?.error?.credential ?? params?.credential ?? "";
    const errorCode = parsed?.error?.code;
    const failed = status === "failed" || !!errorCode;

    return (
        <div className="my-1 rounded-lg border border-border bg-card text-sm" data-aipp-slot="vault-request-tool">
            <button
                type="button"
                className="flex w-full items-center gap-2 px-3 py-2 text-left"
                aria-expanded={expanded}
                onClick={() => setExpanded((value) => !value)}
                disabled={status === "executing"}
            >
                {status === "executing" ? <Loader2 className="h-4 w-4 shrink-0 animate-spin text-muted-foreground" /> : <KeyRound className="h-4 w-4 shrink-0 text-muted-foreground" />}
                <span className="shrink-0 font-mono text-xs font-semibold">{method}</span>
                <span className="min-w-0 flex-1 truncate font-mono text-xs">{url}</span>
                <span className="shrink-0 text-xs text-muted-foreground">
                    {status === "executing" ? "使用凭据中…" : failed ? (ERROR_HINTS[errorCode ?? ""] ?? "失败") : parsed?.status}
                </span>
                {status !== "executing" && (expanded ? <ChevronDown className="h-4 w-4 shrink-0" /> : <ChevronRight className="h-4 w-4 shrink-0" />)}
            </button>
            {expanded && (
                <div className="space-y-2 border-t border-border px-3 py-2">
                    <p className="text-xs text-muted-foreground">凭据 <code className="font-mono">{credential}</code> · 凭据值由 AIPP 注入，助手看不到</p>
                    {failed ? (
                        <p className="whitespace-pre-wrap break-all text-xs">{parsed?.error?.message ?? call?.error ?? "失败"}</p>
                    ) : (
                        <pre className="max-h-80 overflow-auto whitespace-pre-wrap break-all rounded bg-muted/40 p-2 font-mono text-xs">
                            {parsed?.body || "（空响应）"}
                            {parsed?.truncated ? "\n…（响应过长，已截断）" : ""}
                        </pre>
                    )}
                </div>
            )}
        </div>
    );
}
