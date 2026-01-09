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
                <IconButton icon={<Info className="h-4 w-4" />} onClick={() => {}} border />
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
                                        按模型分组
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
                                        总消息数
                                    </p>
                                    <p className="text-xl font-semibold">
                                        {formatNumber(stats.message_count)}
                                    </p>
                                </div>
                                <div className="text-center">
                                    <p className="text-sm text-muted-foreground mb-1">
                                        使用模型数
                                    </p>
                                    <p className="text-xl font-semibold">
                                        {stats.by_model.length}
                                    </p>
                                </div>
                            </div>
                        </div>
                    </div>
                )}
            </DialogContent>
        </Dialog>
    );
}
