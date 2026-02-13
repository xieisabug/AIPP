import { useState } from "react";
import { Popover, PopoverTrigger, PopoverContent } from "@/components/ui/popover";
import IconButton from "../IconButton";
import { Info } from "lucide-react";

interface MessageTokenTooltipProps {
    tokenCount: number;
    inputTokenCount: number;
    outputTokenCount: number;
    messageType: string;
    ttftMs?: number | null;
    tps?: number | null;
    onOpenChange?: (open: boolean) => void;
}

export function MessageTokenTooltip({
    tokenCount,
    inputTokenCount,
    outputTokenCount,
    messageType,
    ttftMs,
    tps,
    onOpenChange,
}: MessageTokenTooltipProps) {
    // 只在 response 类型消息上显示
    if (messageType !== "response") {
        return null;
    }

    const [isOpen, setIsOpen] = useState(false);

    const formatNumber = (num: number) => {
        return new Intl.NumberFormat("en-US").format(num);
    };

    const formatDuration = (ms: number) => {
        if (ms < 1000) return `${ms.toFixed(0)}ms`;
        return `${(ms / 1000).toFixed(2)}s`;
    };

    const formatTps = (tps: number) => {
        return tps.toFixed(1);
    };

    const safeTtftMs =
        typeof ttftMs === "number" && Number.isFinite(ttftMs) ? ttftMs : 0;
    const safeTps = typeof tps === "number" && Number.isFinite(tps) ? tps : 0;
    const hasPerformanceMetrics = true;

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
            <PopoverContent side="top" align="start" className="w-auto">
                <div className="space-y-2">
                    <div className="flex justify-between items-center gap-4 border-b pb-2">
                        <span className="text-sm font-medium">总 Token</span>
                        <span className="text-sm font-bold">
                            {formatNumber(tokenCount)}
                        </span>
                    </div>

                    <div className="space-y-1 text-sm">
                        <div className="flex justify-between gap-4">
                            <span className="text-muted-foreground">输入:</span>
                            <span className="font-medium">
                                {formatNumber(inputTokenCount)}
                            </span>
                        </div>
                        <div className="flex justify-between gap-4">
                            <span className="text-muted-foreground">输出:</span>
                            <span className="font-medium">
                                {formatNumber(outputTokenCount)}
                            </span>
                        </div>
                    </div>

                    {/* 性能指标 */}
                    {hasPerformanceMetrics && (
                        <>
                            <div className="border-t pt-2 mt-2">
                                <div className="text-xs font-medium text-muted-foreground mb-1">性能指标</div>
                            </div>
                            <div className="space-y-1 text-sm">
                                {ttftMs !== null && ttftMs !== undefined && (
                                    <div className="flex justify-between gap-4">
                                        <span className="text-muted-foreground">首字延迟 (TTFT):</span>
                                        <span className="font-medium">{formatDuration(safeTtftMs)}</span>
                                    </div>
                                )}
                                <div className="flex justify-between gap-4">
                                    <span className="text-muted-foreground">生成速度 (TPS):</span>
                                    <span className="font-medium">{formatTps(safeTps)} tok/s</span>
                                </div>
                            </div>
                        </>
                    )}
                </div>
            </PopoverContent>
        </Popover>
    );
}
