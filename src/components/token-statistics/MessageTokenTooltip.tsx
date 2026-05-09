import { useEffect, useState } from "react";
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover";
import { Badge } from "@/components/ui/badge";
import IconButton from "../IconButton";
import { Info } from "lucide-react";
import { tokenStatisticsService } from "@/services/tokenStatisticsService";
import type { MessageTokenStats } from "@/data/Conversation";

interface MessageTokenTooltipProps {
    messageId: number;
    messageType: string;
    onOpenChange?: (open: boolean) => void;
}

export function MessageTokenTooltip({
    messageId,
    messageType,
    onOpenChange,
}: MessageTokenTooltipProps) {
    const [isOpen, setIsOpen] = useState(false);
    const [stats, setStats] = useState<MessageTokenStats | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (!isOpen) {
            return;
        }

        setLoading(true);
        setError(null);
        tokenStatisticsService
            .getMessageTokenStats(messageId)
            .then((nextStats) => {
                setStats(nextStats);
                setLoading(false);
            })
            .catch((err) => {
                setError(err.message || "加载失败");
                setLoading(false);
            });
    }, [isOpen, messageId]);

    const formatNumber = (num: number) => new Intl.NumberFormat("en-US").format(num);

    const formatDuration = (ms: number) => {
        if (ms < 1000) return `${ms.toFixed(0)}ms`;
        return `${(ms / 1000).toFixed(2)}s`;
    };

    const formatTps = (tps: number) => tps.toFixed(1);

    const formatDateTime = (date: Date | null | undefined) => {
        if (!date) return "未知";
        const d = date instanceof Date ? date : new Date(date);
        return d.toLocaleString("zh-CN", {
            year: "numeric",
            month: "2-digit",
            day: "2-digit",
            hour: "2-digit",
            minute: "2-digit",
            second: "2-digit",
            hour12: false,
        });
    };

    const usageSourceLabel = stats?.usage_source === "estimated" ? "估算" : "精确";

    if (messageType !== "response") {
        return null;
    }

    return (
        <Popover
            open={isOpen}
            onOpenChange={(open) => {
                setIsOpen(open);
                onOpenChange?.(open);
            }}
        >
            <PopoverTrigger asChild>
                <IconButton icon={<Info className="h-4 w-4 text-icon" />} onClick={() => {}} />
            </PopoverTrigger>
            <PopoverContent side="top" align="start" className="w-80">
                {loading ? (
                    <div className="text-sm text-muted-foreground">加载中...</div>
                ) : error ? (
                    <div className="text-sm text-destructive">{error}</div>
                ) : stats ? (
                    <div className="space-y-3">
                        <div className="flex items-center justify-between gap-3 border-b pb-2">
                            <span className="text-sm font-medium">消息 Token</span>
                            <Badge variant="secondary">{usageSourceLabel}</Badge>
                        </div>

                        <div className="space-y-1 text-sm">
                            <div className="flex justify-between gap-4">
                                <span className="text-muted-foreground">总 Token</span>
                                <span className="font-medium">{formatNumber(stats.total_tokens)}</span>
                            </div>
                            <div className="flex justify-between gap-4">
                                <span className="text-muted-foreground">输入</span>
                                <span className="font-medium">{formatNumber(stats.input_tokens)}</span>
                            </div>
                            <div className="flex justify-between gap-4">
                                <span className="text-muted-foreground">输出</span>
                                <span className="font-medium">{formatNumber(stats.output_tokens)}</span>
                            </div>
                            {stats.thought_tokens > 0 && (
                                <div className="flex justify-between gap-4">
                                    <span className="text-muted-foreground">思考 Token</span>
                                    <span className="font-medium">{formatNumber(stats.thought_tokens)}</span>
                                </div>
                            )}
                            {stats.cached_read_tokens > 0 && (
                                <div className="flex justify-between gap-4">
                                    <span className="text-muted-foreground">缓存读取</span>
                                    <span className="font-medium">{formatNumber(stats.cached_read_tokens)}</span>
                                </div>
                            )}
                            {stats.cached_write_tokens > 0 && (
                                <div className="flex justify-between gap-4">
                                    <span className="text-muted-foreground">缓存写入</span>
                                    <span className="font-medium">{formatNumber(stats.cached_write_tokens)}</span>
                                </div>
                            )}
                        </div>

                        {(stats.ttft_ms !== null && stats.ttft_ms !== undefined) || stats.tps ? (
                            <div className="space-y-1 border-t pt-2 text-sm">
                                <div className="text-xs font-medium text-muted-foreground">性能指标</div>
                                {stats.ttft_ms !== null && stats.ttft_ms !== undefined && (
                                    <div className="flex justify-between gap-4">
                                        <span className="text-muted-foreground">首字延迟</span>
                                        <span className="font-medium">{formatDuration(stats.ttft_ms)}</span>
                                    </div>
                                )}
                                {stats.tps !== null && stats.tps !== undefined && (
                                    <div className="flex justify-between gap-4">
                                        <span className="text-muted-foreground">生成速度</span>
                                        <span className="font-medium">{formatTps(stats.tps)} tok/s</span>
                                    </div>
                                )}
                            </div>
                        ) : null}

                        {(stats.start_time || stats.finish_time) && (
                            <div className="space-y-1 border-t pt-2 text-sm">
                                <div className="text-xs font-medium text-muted-foreground">时间信息</div>
                                {stats.start_time && (
                                    <div className="flex justify-between gap-4">
                                        <span className="text-muted-foreground">开始时间</span>
                                        <span className="font-medium">{formatDateTime(stats.start_time)}</span>
                                    </div>
                                )}
                                {stats.finish_time && (
                                    <div className="flex justify-between gap-4">
                                        <span className="text-muted-foreground">完成时间</span>
                                        <span className="font-medium">{formatDateTime(stats.finish_time)}</span>
                                    </div>
                                )}
                            </div>
                        )}
                    </div>
                ) : (
                    <div className="text-sm text-muted-foreground">暂无统计</div>
                )}
            </PopoverContent>
        </Popover>
    );
}
