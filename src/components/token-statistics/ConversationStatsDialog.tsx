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
import { describeInvokeError, tokenStatisticsService } from "@/services/tokenStatisticsService";
import type { AcpConversationSessionState, ConversationTokenStats } from "@/data/Conversation";
import { TokenUsageDisplay } from "./TokenUsageDisplay";
import { formatUsageDollars } from "./formatUsageDollars";
import { Badge } from "@/components/ui/badge";
import { Progress } from "@/components/ui/progress";

interface ConversationStatsDialogProps {
    /** 单个会话的统计；与 groupId 二选一 */
    conversationId?: string;
    /** 管家群聊的统计：成员执行会话 + 调度模型调用 */
    groupId?: number;
    showCacheActivity?: boolean;
    externalOpen?: boolean;
    onExternalOpenChange?: (open: boolean) => void;
}

type CacheActivityRow = { id: number; checkpoint: boolean; input: number; cached: number | null };

const AUXILIARY_LABELS: Record<string, string> = {
    context_compaction: "上下文压缩",
    group_dispatch: "群聊调度",
};

const formatNumber = (num: number) => new Intl.NumberFormat("en-US").format(num);

const formatDateTime = (date: Date | string | null | undefined) => {
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

function formatDuration(ms: number): string {
    if (ms < 1000) return `${ms.toFixed(0)}ms`;
    return `${(ms / 1000).toFixed(2)}s`;
}

function formatCacheActivity(row: CacheActivityRow): string {
    if (row.checkpoint) return "上下文检查点";
    if (row.cached == null) return `#${row.id} · 未提供缓存命中量（输入 ${row.input.toLocaleString()}）`;
    const rate = (100 * Math.min(row.cached, row.input)) / row.input;
    return `#${row.id} · 缓存命中 ${row.cached.toLocaleString()} / 输入 ${row.input.toLocaleString()}（${rate.toFixed(1)}%）`;
}

/** 总量的构成：本会话自身 + 委派 + 辅助调用，各项互不重叠 */
function usageComposition(stats: ConversationTokenStats, isGroup: boolean) {
    const auxiliary = stats.auxiliary ?? [];
    const delegatedTokens = stats.delegated?.run_count ? stats.delegated.total_tokens : 0;
    const auxiliaryTokens = auxiliary.reduce((sum, entry) => sum + entry.total_tokens, 0);
    const rows = [
        {
            key: "own",
            label: isGroup ? "成员回复" : "本会话回复",
            value: Math.max(stats.total_tokens - delegatedTokens - auxiliaryTokens, 0),
        },
    ];
    if (stats.delegated && stats.delegated.run_count > 0) {
        rows.push({
            key: "delegated",
            label: `助手协作（${formatNumber(stats.delegated.run_count)} 次委派，含多层转派）`,
            value: stats.delegated.total_tokens,
        });
    }
    for (const entry of auxiliary) {
        rows.push({
            key: `auxiliary-${entry.purpose}`,
            label: `${AUXILIARY_LABELS[entry.purpose] ?? entry.purpose}（${formatNumber(entry.call_count)} 次调用）`,
            value: entry.total_tokens,
        });
    }
    return rows;
}

export function ConversationStatsDialog({
    conversationId,
    groupId,
    showCacheActivity = false,
    externalOpen,
    onExternalOpenChange,
}: ConversationStatsDialogProps) {
    const isGroup = groupId !== undefined;
    const [activity, setActivity] = useState<CacheActivityRow[]>([]);
    const [activityError, setActivityError] = useState("");
    const [internalOpen, setInternalOpen] = useState(false);
    const open = externalOpen !== undefined ? externalOpen : internalOpen;
    const setOpen = onExternalOpenChange || setInternalOpen;
    const [stats, setStats] = useState<ConversationTokenStats | null>(null);
    const [acpSessionState, setAcpSessionState] = useState<AcpConversationSessionState | null>(null);
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    useEffect(() => {
        if (!open || !showCacheActivity || !conversationId || isGroup) return;
        let current = true;
        setActivity([]);
        setActivityError("");
        invoke<CacheActivityRow[]>("get_butler_cache_activity", { conversationId: Number(conversationId) })
            .then(rows => { if (current) setActivity(rows); })
            .catch(error => { if (current) setActivityError(describeInvokeError(error, "读取缓存命中记录失败")); });
        return () => { current = false; };
    }, [open, conversationId, showCacheActivity, isGroup]);

    useEffect(() => {
        if (!open) {
            setAcpSessionState(null);
            return;
        }
        if (!isGroup && !conversationId) return;
        let current = true;
        setLoading(true);
        setError(null);
        const statsRequest = groupId !== undefined
            ? tokenStatisticsService.getGroupTokenStats(groupId)
            : tokenStatisticsService.getConversationTokenStats(conversationId!);
        const acpRequest = isGroup
            ? Promise.resolve(null)
            : invoke<AcpConversationSessionState | null>("get_acp_session_state", {
                conversationId: Number(conversationId),
            }).catch(() => null);
        Promise.all([statsRequest, acpRequest])
            .then(([statsData, acpState]) => {
                if (!current) return;
                setStats(statsData);
                setAcpSessionState(acpState);
                setLoading(false);
            })
            .catch((err) => {
                if (!current) return;
                console.error("Failed to load conversation stats:", err);
                setError(describeInvokeError(err, isGroup ? "加载群聊用量统计失败" : "加载对话用量统计失败"));
                setLoading(false);
            });
        return () => { current = false; };
    }, [open, conversationId, groupId, isGroup]);

    const hasAcpSessionUsage =
        acpSessionState?.context_tokens_used !== null
        && acpSessionState?.context_tokens_used !== undefined
        && acpSessionState?.context_window_size !== null
        && acpSessionState?.context_window_size !== undefined;

    const context = stats?.context ?? null;
    const contextPercent = context && context.context_window_size > 0
        ? (context.latest_input_tokens / context.context_window_size) * 100
        : 0;
    const composition = stats ? usageComposition(stats, isGroup) : [];
    const messageTypes = stats ? [
        { label: "系统", value: stats.system_message_count },
        { label: "用户", value: stats.user_message_count },
        { label: "AI回复", value: stats.response_message_count },
        { label: "推理", value: stats.reasoning_message_count },
        { label: "工具结果", value: stats.tool_result_message_count },
        { label: "错误", value: stats.error_message_count ?? 0 },
        ...((stats.other_message_count ?? 0) > 0 ? [{ label: "其他", value: stats.other_message_count ?? 0 }] : []),
    ] : [];
    const hasTtft = stats?.avg_ttft_ms !== null && stats?.avg_ttft_ms !== undefined;
    const hasTps = stats?.avg_tps !== null && stats?.avg_tps !== undefined;
    const title = isGroup ? "群聊用量" : "对话信息";

    return (
        <Dialog open={open} onOpenChange={setOpen}>
            <DialogTrigger asChild>
                <IconButton icon={<Info className="h-4 w-4 text-icon" />} onClick={() => { }} border title={title} />
            </DialogTrigger>
            <DialogContent className="max-h-[85vh] flex flex-col sm:min-w-2xl sm:max-w-[50vw]">
                <DialogHeader>
                    <DialogTitle>{title}</DialogTitle>
                    <DialogDescription>
                        {isGroup ? "该群聊所有成员的执行记录与调度用量" : "该对话的详细信息和统计"}
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
                                        输入含缓存读写，输出含思考；总量含助手协作与辅助模型调用
                                    </p>
                                </div>
                                {stats.estimated_message_count > 0 && (
                                    <Badge variant="secondary">
                                        含估算 {formatNumber(stats.estimated_message_count)} 条
                                    </Badge>
                                )}
                            </div>

                            {(stats.start_time || stats.finish_time) && (
                                <div className="pt-4 border-t">
                                    <h4 className="text-sm font-medium mb-3">时间信息</h4>
                                    <div className="grid grid-cols-2 gap-3">
                                        <div className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">开始时间</p>
                                            <p className="text-sm font-semibold">{formatDateTime(stats.start_time)}</p>
                                        </div>
                                        <div className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">最近完成时间</p>
                                            <p className="text-sm font-semibold">{formatDateTime(stats.finish_time)}</p>
                                        </div>
                                    </div>
                                </div>
                            )}

                            {showCacheActivity && !isGroup && <div className="mb-3 text-xs" data-aipp-slot="conversation-stats-cache-activity">
                                <h3 className="mb-2 font-medium">最近请求的缓存命中</h3>
                                {activityError || (!activity.length && "暂无请求记录")}
                                <div className="max-h-40 overflow-y-auto space-y-1">
                                    {activity.map(row => <div key={row.id}>{formatCacheActivity(row)}</div>)}
                                </div>
                            </div>}

                            <TokenUsageDisplay
                                total={stats.total_tokens}
                                input={stats.input_tokens}
                                output={stats.output_tokens}
                                thought={stats.thought_tokens}
                                cachedInput={stats.cached_input_tokens ?? stats.cached_read_tokens}
                                cachedWrite={stats.cached_write_tokens}
                                showPercentage={true}
                            />

                            {composition.length > 1 && (
                                <div className="space-y-1.5 text-sm" data-aipp-slot="conversation-stats-composition">
                                    <p className="text-xs font-medium text-muted-foreground">总量构成</p>
                                    {composition.map((row) => (
                                        <div
                                            key={row.key}
                                            className="flex justify-between text-muted-foreground"
                                            data-aipp-slot={row.key === "delegated" ? "conversation-stats-delegated" : undefined}
                                        >
                                            <span>其中{row.label}</span>
                                            <span className="font-medium text-foreground">
                                                {formatNumber(row.value)} tokens
                                            </span>
                                        </div>
                                    ))}
                                </div>
                            )}

                            {stats.opencode_go_allowance != null && (
                                <div className="flex justify-between text-sm">
                                    <span>OpenCode Go 订阅额度累计扣减</span>
                                    <span>{formatUsageDollars(stats.opencode_go_allowance)}</span>
                                </div>
                            )}

                            {context && (
                                <div className="space-y-3 rounded-lg border p-4" data-aipp-slot="conversation-stats-context">
                                    <div className="flex items-center justify-between">
                                        <h4 className="text-sm font-medium">当前上下文</h4>
                                        <span className="text-xs text-muted-foreground">
                                            按最近一次请求的输入计算
                                        </span>
                                    </div>
                                    <div className="flex justify-between text-sm">
                                        <span className="text-muted-foreground">最近一次请求输入 / 上下文窗口</span>
                                        <span className="font-medium">
                                            {formatNumber(context.latest_input_tokens)} / {formatNumber(context.context_window_size)}
                                            <span className="ml-1 text-muted-foreground">({contextPercent.toFixed(1)}%)</span>
                                        </span>
                                    </div>
                                    <Progress value={Math.min(contextPercent, 100)} className="h-2" />
                                    <div className="grid grid-cols-2 gap-3 text-sm">
                                        <div className="flex justify-between">
                                            <span className="text-muted-foreground">自动压缩</span>
                                            <span className="font-medium">
                                                {context.compaction_enabled
                                                    ? `超过 ${formatNumber(context.compaction_trigger)} 触发`
                                                    : "已关闭"}
                                            </span>
                                        </div>
                                        <div className="flex justify-between">
                                            <span className="text-muted-foreground">已压缩</span>
                                            <span className="font-medium">{formatNumber(context.checkpoint_count)} 次</span>
                                        </div>
                                    </div>
                                </div>
                            )}

                            {!isGroup && hasAcpSessionUsage && (
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

                            {stats.by_model.length > 0 && (
                                <div className="space-y-4">
                                    <h3 className="text-lg font-semibold">模型</h3>
                                    <div className="space-y-3">
                                        {stats.by_model.map((model) => (
                                            <div
                                                key={`${model.model_id ?? "none"}-${model.model_name}`}
                                                className="border rounded-lg p-4"
                                            >
                                                <div className="flex justify-between items-start mb-3">
                                                    <div>
                                                        <p className="font-medium">{model.model_name}</p>
                                                        <p className="text-sm text-muted-foreground">
                                                            {formatNumber(model.request_count)} 次调用
                                                        </p>
                                                    </div>
                                                    <div className="text-right">
                                                        <p className="text-2xl font-bold">
                                                            {formatNumber(model.total_tokens)}
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
                                                    cachedInput={model.cached_input_tokens ?? model.cached_read_tokens}
                                                    cachedWrite={model.cached_write_tokens}
                                                    compact={true}
                                                />
                                            </div>
                                        ))}
                                    </div>
                                </div>
                            )}

                            <div className="grid grid-cols-2 gap-4 pt-4 border-t">
                                <div className="text-center">
                                    <p className="text-sm text-muted-foreground mb-1">模型</p>
                                    <p className="text-xl font-semibold">{stats.by_model.length}</p>
                                </div>
                                <div className="text-center">
                                    <p className="text-sm text-muted-foreground mb-1">
                                        {isGroup ? "成员执行记录消息" : "本会话消息"}
                                    </p>
                                    <p className="text-xl font-semibold">{formatNumber(stats.message_count)}</p>
                                </div>
                            </div>

                            <div className="pt-4 border-t">
                                <h4 className="text-sm font-medium mb-1">消息类型统计</h4>
                                <p className="mb-3 text-xs text-muted-foreground">只统计{isGroup ? "成员执行会话" : "本会话"}自身的消息，不含协作对象的消息</p>
                                <div className="grid gap-3" style={{ gridTemplateColumns: `repeat(${messageTypes.length}, minmax(0, 1fr))` }}>
                                    {messageTypes.map((item) => (
                                        <div key={item.label} className="text-center">
                                            <p className="text-xs text-muted-foreground mb-1">{item.label}</p>
                                            <p className="text-lg font-semibold">{formatNumber(item.value)}</p>
                                        </div>
                                    ))}
                                </div>
                            </div>

                            <div className="pt-4 border-t" data-aipp-slot="conversation-stats-performance">
                                <h4 className="text-sm font-medium mb-1">性能指标</h4>
                                <p className="mb-3 text-xs text-muted-foreground">
                                    含助手协作的回复；Agent 回合包含工具执行时间，不计入生成速度
                                </p>
                                <div className="grid grid-cols-2 gap-3">
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">平均首字延迟</p>
                                        <p className="text-lg font-semibold">
                                            {hasTtft ? formatDuration(stats.avg_ttft_ms!) : "—"}
                                        </p>
                                        <p className="text-xs text-muted-foreground">
                                            {hasTtft ? `${formatNumber(stats.ttft_sample_count ?? 0)} 次请求` : "暂无数据"}
                                        </p>
                                    </div>
                                    <div className="text-center">
                                        <p className="text-xs text-muted-foreground mb-1">平均生成速度</p>
                                        <p className="text-lg font-semibold">
                                            {hasTps ? `${stats.avg_tps!.toFixed(1)} tok/s` : "—"}
                                        </p>
                                        <p className="text-xs text-muted-foreground">
                                            {hasTps ? `${formatNumber(stats.tps_sample_count ?? 0)} 次请求，按输出总量 ÷ 生成总时长` : "暂无数据"}
                                        </p>
                                    </div>
                                </div>
                            </div>
                        </div>
                    </div>
                )}
            </DialogContent>
        </Dialog>
    );
}
