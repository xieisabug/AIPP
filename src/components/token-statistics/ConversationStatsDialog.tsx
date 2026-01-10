import { useState, useEffect } from "react";
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
import type { ConversationTokenStats } from "@/data/Conversation";
import { TokenUsageDisplay } from "./TokenUsageDisplay";

interface ConversationStatsDialogProps {
    conversationId: string;
}

export function ConversationStatsDialog({
    conversationId,
}: ConversationStatsDialogProps) {
    const [open, setOpen] = useState(false);
    const [stats, setStats] = useState<ConversationTokenStats | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (open && conversationId) {
            setLoading(true);
            setError(null);

            tokenStatisticsService
                .getConversationTokenStats(conversationId)
                .then((data) => {
                    setStats(data);
                    setLoading(false);
                })
                .catch((err) => {
                    console.error("Failed to load conversation stats:", err);
                    setError(err.message || "Failed to load statistics");
                    setLoading(false);
                });
        }
    }, [open, conversationId]);

    const formatNumber = (num: number) => {
        return new Intl.NumberFormat("en-US").format(num);
    };

    return (
        <Dialog open={open} onOpenChange={setOpen}>
            <DialogTrigger asChild>
                <IconButton icon={<Info className="h-4 w-4" />} onClick={() => { }} border />
            </DialogTrigger>
            <DialogContent className="max-w-2xl max-h-[85vh] flex flex-col">
                <DialogHeader>
                    <DialogTitle>对话 Token 统计</DialogTitle>
                    <DialogDescription>
                        该对话的 Token 使用明细
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
                            {/* Total Token Usage Display */}
                            <TokenUsageDisplay
                                total={stats.total_tokens}
                                input={stats.input_tokens}
                                output={stats.output_tokens}
                                showPercentage={true}
                            />

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
                                                    output={
                                                        model.output_tokens
                                                    }
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
