import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
    Dialog,
    DialogContent,
    DialogDescription,
    DialogHeader,
    DialogTitle,
    DialogTrigger,
} from "@/components/ui/dialog";
import IconButton from "../IconButton";
import { Info } from "lucide-react";
import { tokenStatisticsService } from "@/services/tokenStatisticsService";
import type { AcpConversationSessionState, ConversationTokenStats } from "@/data/Conversation";
import { TokenUsageDisplay } from "./TokenUsageDisplay";
import { Badge } from "@/components/ui/badge";

interface ConversationStatsDialogProps {
    conversationId: string;
    externalOpen?: boolean;
    onExternalOpenChange?: (open: boolean) => void;
}

export function ConversationStatsDialog({
    conversationId,
    externalOpen,
    onExternalOpenChange,
}: ConversationStatsDialogProps) {
    const [internalOpen, setInternalOpen] = useState(false);
    const open = externalOpen !== undefined ? externalOpen : internalOpen;
    const setOpen = onExternalOpenChange || setInternalOpen;
    const [stats, setStats] = useState<ConversationTokenStats | null>(null);
    const [acpSessionState, setAcpSessionState] = useState<AcpConversationSessionState | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (open && conversationId) {
            setLoading(true);
            setError(null);
            Promise.all([
                tokenStatisticsService.getConversationTokenStats(conversationId),
                invoke<AcpConversationSessionState | null>("get_acp_session_state", {
                    conversationId: Number(conversationId),
                }).catch(() => null),
            ])
                .then(([statsData, acpState]) => {
                    setStats(statsData);
                    setAcpSessionState(acpState);
                    setLoading(false);
                })
                .catch((err) => {
                    console.error("Failed to load conversation stats:", err);
                    setError(err.message || "Failed to load statistics");
                    setLoading(false);
                });
        } else if (!open) {
            setAcpSessionState(null);
        }
    }, [open, conversationId]);

    const formatNumber = (num: number) => {
        return new Intl.NumberFormat("en-US").format(num);
    };

    const formatDateTime = (date: Date | null | undefined) => {
        if (!date) return "未知";
        const d = date instanceof Date ? date : new Date(date);
        return d.toLocaleString('zh-CN', {
            year: 'numeric',
            month: '2-digit',
            day: '2-digit',
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
            hour12: false
        });
    };

    const formatCurrency = (amount: number, currency: string) => {
        try {
            return new Intl.NumberFormat("en-US", {
                style: "currency",
                currency,
                maximumFractionDigits: 4,
            }).format(amount);
        } catch {
            return `${amount.toFixed(4)} ${currency}`;
        }
    };

    const hasAcpSessionUsage =
        acpSessionState?.context_tokens_used !== null
        && acpSessionState?.context_tokens_used !== undefined
        && acpSessionState?.context_window_size !== null
        && acpSessionState?.context_window_size !== undefined;

    return (
        <Dialog open={open} onOpenChange={setOpen}>
            <DialogTrigger asChild>
                <IconButton icon={<Info className="h-4 w-4 text-icon" />} onClick={() => { }} border />
            </DialogTrigger>
            <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col">
                <DialogHeader>
                    <DialogTitle>对话信息</DialogTitle>
                    <DialogDescription>
                        该对话的详细信息和统计
                    </DialogDescription>
                </DialogHeader>

                {loading && (
                    <div className="flex-1 flex items-center justify-center px-6">
                        <p className="text-muted-foreground">加载统计中...</p>
                    </div>
                )}

                {error && (
                    <div className="flex-1 flex items-center justify-center px-6">
                        <p className="text-destructive">{error}</p>
                    </div>
                )}

                {stats && !loading && !error && (
                    <div className="flex-1 overflow-y-auto px-6 pb-6">
                        <div className="space-y-6">
                            <div className="flex items-center justify-between gap-3">
                                <div>
                                    <h3 className="text-lg font-semibold">Token 用量</h3>
                                    <p className="text-sm text-muted-foreground">
                                        对话级 token、缓存与会话 usage 统计
                                    </p>
                                </div>
                                {stats.estimated_message_count > 0 && (
                                    <Badge variant="secondary">
                                        含估算 {formatNumber(stats.estimated_message_count)} 条
                                    </Badge>
                                )}
                            </div>

                            {/* 时间戳信息 */}
                            {(stats.start_time || stats.finish_time) && (
                                <div className="pt-4 border-t">
                                    <h4 className="text-sm font-medium mb-3">时间信息</h4>
                                    <div className="grid grid-cols-2 gap-3">
                                        <div className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">
                                                开始时间
                                            </p>
                                            <p className="text-sm font-semibold">
                                                {formatDateTime(stats.start_time)}
                                            </p>
                                        </div>
                                        <div className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">
                                                完成时间
                                            </p>
                                            <p className="text-sm font-semibold">
                                                {formatDateTime(stats.finish_time)}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            )}

                            {/* Total Token Usage Display */}
                            <TokenUsageDisplay
                                total={stats.total_tokens}
                                input={stats.input_tokens}
                                output={stats.output_tokens}
                                thought={stats.thought_tokens}
                                cachedRead={stats.cached_read_tokens}
                                cachedWrite={stats.cached_write_tokens}
                                showPercentage={true}
                            />

                            {hasAcpSessionUsage && (
                                <div className="space-y-3 rounded-lg border p-4">
                                    <div className="flex items-center justify-between">
                                        <h4 className="text-sm font-medium">ACP 会话 Usage</h4>
                                        <span className="text-xs text-muted-foreground">
                                            实时会话上下文
                                        </span>
                                    </div>
                                    <div className="grid grid-cols-2 gap-3">
                                        <div className="rounded-md bg-muted/40 p-3 text-center">
                                            <p className="text-xs text-muted-foreground mb-1">已用上下文</p>
                                            <p className="text-lg font-semibold">
                                                {formatNumber(acpSessionState?.context_tokens_used ?? 0)}
                                            </p>
                                        </div>
                                        <div className="rounded-md bg-muted/40 p-3 text-center">
                                            <p className="text-xs text-muted-foreground mb-1">上下文窗口</p>
                                            <p className="text-lg font-semibold">
                                                {formatNumber(acpSessionState?.context_window_size ?? 0)}
                                            </p>
                                        </div>
                                    </div>
                                    {acpSessionState?.session_cost_amount !== null
                                        && acpSessionState?.session_cost_amount !== undefined
                                        && acpSessionState?.session_cost_currency && (
                                        <div className="flex justify-between text-sm">
                                            <span className="text-muted-foreground">累计成本</span>
                                            <span className="font-medium">
                                                {formatCurrency(
                                                    acpSessionState.session_cost_amount,
                                                    acpSessionState.session_cost_currency,
                                                )}
                                            </span>
                                        </div>
                                    )}
                                </div>
                            )}

                            {/* Breakdown by Model */}
                            {stats.by_model.length > 0 && (
                                <div className="space-y-4">
                                    <h3 className="text-lg font-semibold">
                                        模型
                                    </h3>
                                    <div className="space-y-3">
                                        {stats.by_model.map((model) => (
                                            <div
                                                key={model.model_id}
                                                className="border rounded-lg p-4"
                                            >
                                                <div className="flex justify-between items-start mb-3">
                                                    <div>
                                                        <p className="font-medium">
                                                            {model.model_name ||
                                                                `模型 ${model.model_id}`}
                                                        </p>
                                                        <p className="text-sm text-muted-foreground">
                                                            {formatNumber(model.message_count)}{" "}
                                                            条消息
                                                        </p>
                                                    </div>
                                                    <div className="text-right">
                                                        <p className="text-2xl font-bold">
                                                            {formatNumber(
                                                                model.total_tokens,
                                                            )}
                                                        </p>
                                                        <p className="text-sm text-muted-foreground">
                                                            {(model.percentage || 0).toFixed(1)}% 占总计
                                                        </p>
                                                    </div>
                                                </div>

                                                <TokenUsageDisplay
                                                    total={model.total_tokens}
                                                    input={model.input_tokens}
                                                    output={model.output_tokens}
                                                    thought={model.thought_tokens}
                                                    cachedRead={model.cached_read_tokens}
                                                    cachedWrite={model.cached_write_tokens}
                                                    compact={true}
                                                />
                                            </div>
                                        ))}
                                    </div>
                                </div>
                            )}

                            {/* Summary Statistics */}
                            <div className="grid grid-cols-2 gap-4 pt-4 border-t">
                                <div className="text-center">
                                    <p className="text-sm text-muted-foreground mb-1">
                                        模型
                                    </p>
                                    <p className="text-xl font-semibold">
                                        {stats.by_model.length}
                                    </p>
                                </div>
                                <div className="text-center">
                                    <p className="text-sm text-muted-foreground mb-1">
                                        总消息数
                                    </p>
                                    <p className="text-xl font-semibold">
                                        {formatNumber(stats.message_count)}
                                    </p>
                                </div>
                            </div>

                            {/* Message Type Breakdown */}
                            <div className="pt-4 border-t">
                                <h4 className="text-sm font-medium mb-3">消息类型统计</h4>
                                <div className="grid grid-cols-5 gap-3">
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">
                                            系统
                                        </p>
                                        <p className="text-lg font-semibold">
                                            {formatNumber(stats.system_message_count)}
                                        </p>
                                    </div>
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">
                                            用户
                                        </p>
                                        <p className="text-lg font-semibold">
                                            {formatNumber(stats.user_message_count)}
                                        </p>
                                    </div>
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">
                                            AI回复
                                        </p>
                                        <p className="text-lg font-semibold">
                                            {formatNumber(stats.response_message_count)}
                                        </p>
                                    </div>
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">
                                            推理
                                        </p>
                                        <p className="text-lg font-semibold">
                                            {formatNumber(
                                                stats.reasoning_message_count,
                                            )}
                                        </p>
                                    </div>
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">
                                            工具结果
                                        </p>
                                        <p className="text-lg font-semibold">
                                            {formatNumber(
                                                stats.tool_result_message_count,
                                            )}
                                        </p>
                                    </div>
                                </div>
                            </div>

                            {/* Performance Metrics */}
                            {(stats.avg_ttft_ms !== undefined || stats.avg_tps !== undefined) && (
                                <div className="pt-4 border-t">
                                    <h4 className="text-sm font-medium mb-3">性能指标 (响应消息)</h4>
                                    <div className="grid grid-cols-2 gap-3">
                                        <div className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">
                                                平均首字延迟
                                            </p>
                                            <p className="text-lg font-semibold">
                                                {formatDuration(stats.avg_ttft_ms ?? 0)}
                                            </p>
                                        </div>
                                        <div className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">
                                                平均生成速度
                                            </p>
                                            <p className="text-lg font-semibold">
                                                {`${(stats.avg_tps ?? 0).toFixed(1)} tok/s`}
                                            </p>
                                        </div>
                                    </div>
                                </div>
                            )}
                        </div>
                    </div>
                )}
            </DialogContent>
        </Dialog>
    );
}

// 辅助函数：格式化持续时间
function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms.toFixed(0)}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
}
