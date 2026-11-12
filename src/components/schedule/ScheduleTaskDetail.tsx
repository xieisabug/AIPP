import { memo, useMemo, useState } from "react";
import { AlertCircle, ArrowRight, MoreHorizontal, Pencil, Play, RefreshCw, Send, Square, Trash2, User } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ScheduledRun, ScheduledTask, ScheduledTaskLog } from "./scheduleTypes";
import {
    deliverPolicyLabels,
    formatClockTime,
    formatRunDuration,
    formatShortDateTime,
    isProblemStatus,
    logTypeLabels,
    parseToolLogPayload,
    runStatusLabel,
    taskGroup,
    taskGroupLabels,
} from "./scheduleFormat";

export interface ScheduleTaskDetailProps {
    task: ScheduledTask;
    runs: ScheduledRun[];
    selectedRunId: string | null;
    isRunsLoading: boolean;
    logs: ScheduledTaskLog[];
    isLogsLoading: boolean;
    isStarting: boolean;
    isStopping: boolean;
    onSelectRun: (runId: string) => void;
    onRefreshRuns: () => void;
    onToggleEnabled: (enabled: boolean) => void;
    onEdit: () => void;
    onRunNow: () => void;
    onStop: (run: ScheduledRun) => void;
    onDelete: () => void;
    onOpenToolLog: (log: ScheduledTaskLog) => void;
}

/** 状态圆点：只用灰阶区分，失败类用 destructive 作为唯一强调色。 */
export function RunStatusDot({ status }: { status: string }) {
    const base = "inline-block size-2 shrink-0 rounded-full";
    if (status === "running") return <span className={`${base} bg-foreground animate-pulse`} aria-hidden="true" />;
    if (isProblemStatus(status)) return <span className={`${base} bg-destructive`} aria-hidden="true" />;
    if (status === "missed" || status === "cancelled") {
        return <span className={`${base} border border-dashed border-muted-foreground`} aria-hidden="true" />;
    }
    if (status === "quiet") return <span className={`${base} border border-muted-foreground`} aria-hidden="true" />;
    return <span className={`${base} bg-foreground/60`} aria-hidden="true" />;
}

const PROMPT_COLLAPSE_CHARS = 320;
const PROMPT_COLLAPSE_LINES = 7;

function PromptBlock({ text }: { text: string }) {
    const [expanded, setExpanded] = useState(false);
    const value = text.trim();
    const collapsible = value.length > PROMPT_COLLAPSE_CHARS || value.split("\n").length > PROMPT_COLLAPSE_LINES;
    return (
        <div className="min-w-0 space-y-1" data-aipp-slot="schedule-detail-prompt">
            <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-medium text-muted-foreground">每次运行的指令</span>
                {collapsible && (
                    <button
                        type="button"
                        className="text-xs text-muted-foreground hover:text-foreground"
                        aria-expanded={expanded}
                        onClick={() => setExpanded((prev) => !prev)}
                    >
                        {expanded ? "收起" : "展开"}
                    </button>
                )}
            </div>
            <div
                className={`whitespace-pre-wrap break-words rounded-md bg-muted/40 px-3.5 py-3 text-[13px] leading-6 ${
                    collapsible && !expanded ? "line-clamp-7" : "max-h-[28rem] overflow-y-auto thin-scrollbar"
                }`}
            >
                {value}
            </div>
        </div>
    );
}

function PanelHeader({ title, meta, action }: { title: string; meta?: React.ReactNode; action?: React.ReactNode }) {
    return (
        <div className="flex h-10 shrink-0 items-center justify-between gap-2 border-b border-border px-3">
            <div className="flex min-w-0 items-center gap-2">
                <span className="text-xs font-semibold">{title}</span>
                {meta && <span className="truncate text-xs text-muted-foreground">{meta}</span>}
            </div>
            {action}
        </div>
    );
}

function PanelMessage({ children }: { children: React.ReactNode }) {
    return <div className="flex h-full min-h-24 items-center justify-center px-3 text-xs text-muted-foreground">{children}</div>;
}

function MetaItem({ label, children }: { label: string; children: React.ReactNode }) {
    return (
        <div className="min-w-0 space-y-0.5">
            <dt className="text-[11px] text-muted-foreground">{label}</dt>
            <dd className="text-sm break-words">{children}</dd>
        </div>
    );
}

/** 本次运行交给用户的内容（日志里的「产出」）。 */
function runOutput(logs: ScheduledTaskLog[]): string | null {
    for (let i = logs.length - 1; i >= 0; i -= 1) {
        if (logs[i].messageType === "response") return logs[i].content;
    }
    return null;
}

function LogLine({ log, onOpenToolLog }: { log: ScheduledTaskLog; onOpenToolLog: (log: ScheduledTaskLog) => void }) {
    const isError = ["error", "delivery_error", "needs_user", "review_held"].includes(log.messageType);
    const toolPayload = parseToolLogPayload(log);
    return (
        <li className="flex gap-3 px-3 py-2 text-xs">
            <span className="w-14 shrink-0 font-mono text-[11px] leading-5 text-muted-foreground">{formatClockTime(log.createdTime)}</span>
            <div className="min-w-0 flex-1 space-y-0.5">
                <span className={`text-[11px] font-medium leading-5 ${isError ? "text-destructive" : "text-muted-foreground"}`}>
                    {logTypeLabels[log.messageType] ?? log.messageType}
                </span>
                {toolPayload ? (
                    <div className="flex flex-wrap items-center gap-x-2 leading-5">
                        <span>
                            {log.messageType === "tool_result" ? (toolPayload.success ? "执行成功" : "未执行或失败") : "准备执行"}：
                            {toolPayload.serverName ?? "-"} / {toolPayload.toolName ?? "-"}
                        </span>
                        <button
                            type="button"
                            className="text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                            onClick={() => onOpenToolLog(log)}
                        >
                            参数 / 返回
                        </button>
                    </div>
                ) : log.messageType === "response" || log.messageType === "task_prompt" ? (
                    <div className="line-clamp-3 whitespace-pre-wrap break-words leading-5 text-muted-foreground">{log.content}</div>
                ) : (
                    <div className={`whitespace-pre-wrap break-words leading-5 ${isError ? "text-destructive" : ""}`}>{log.content}</div>
                )}
            </div>
        </li>
    );
}

function ScheduleTaskDetail({
    task,
    runs,
    selectedRunId,
    isRunsLoading,
    logs,
    isLogsLoading,
    isStarting,
    isStopping,
    onSelectRun,
    onRefreshRuns,
    onToggleEnabled,
    onEdit,
    onRunNow,
    onStop,
    onDelete,
    onOpenToolLog,
}: ScheduleTaskDetailProps) {
    const selectedRun = runs.find((run) => run.runId === selectedRunId) ?? null;
    const runningRun = runs.find((run) => run.status === "running" && run.localRunning) ?? null;
    const group = taskGroup(task);
    const output = useMemo(() => runOutput(logs), [logs]);
    const endText = [
        task.endsAt ? `${formatShortDateTime(task.endsAt)} 截止` : null,
        task.maxRuns ? `最多 ${task.maxRuns} 次` : null,
    ]
        .filter(Boolean)
        .join("，");

    return (
        // 最小高度兜底：窗口很矮时退回外层整页滚动，而不是把两栏压扁
        <div className="flex h-full min-h-[760px] flex-col gap-4" data-aipp-slot="schedule-task-detail">
            <section className="shrink-0 space-y-5 rounded-lg border border-border p-5" data-aipp-slot="schedule-detail-overview">
                <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                            <h2 className="truncate text-lg font-semibold">{task.name}</h2>
                            {group !== "active" && (
                                <Badge
                                    variant={group === "attention" ? "destructive" : "secondary"}
                                    className="shrink-0 text-[11px] font-normal"
                                >
                                    {taskGroupLabels[group]}
                                </Badge>
                            )}
                        </div>
                        <p className="mt-1.5 text-sm">{task.scheduleText}</p>
                        {task.upcomingText.length > 0 && (
                            <p className="mt-1 text-xs text-muted-foreground" data-aipp-slot="schedule-detail-upcoming">
                                接下来：{task.upcomingText.join("、")}
                            </p>
                        )}
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                        <label className="flex items-center gap-2 text-xs text-muted-foreground">
                            启用
                            <Switch checked={task.enabled} onCheckedChange={onToggleEnabled} aria-label="启用任务" />
                        </label>
                        {runningRun ? (
                            <Button variant="outline" size="sm" onClick={() => onStop(runningRun)} disabled={isStopping}>
                                <Square className="h-3.5 w-3.5" />
                                {isStopping ? "停止中…" : "停止"}
                            </Button>
                        ) : (
                            <Button variant="outline" size="sm" onClick={onRunNow} disabled={isStarting}>
                                <Play className="h-3.5 w-3.5" />
                                {isStarting ? "启动中…" : "立即运行"}
                            </Button>
                        )}
                        <Button variant="outline" size="icon" className="size-8" onClick={onEdit} title="编辑" aria-label="编辑">
                            <Pencil className="h-3.5 w-3.5" />
                        </Button>
                        <DropdownMenu>
                            <DropdownMenuTrigger asChild>
                                <Button variant="ghost" size="icon" className="size-8" title="更多" aria-label="更多操作">
                                    <MoreHorizontal className="h-4 w-4" />
                                </Button>
                            </DropdownMenuTrigger>
                            <DropdownMenuContent align="end">
                                <DropdownMenuItem className="text-destructive focus:text-destructive" onSelect={onDelete}>
                                    <Trash2 className="h-4 w-4" />
                                    删除任务
                                </DropdownMenuItem>
                            </DropdownMenuContent>
                        </DropdownMenu>
                    </div>
                </div>

                {(task.pausedReason && !task.enabled) || task.consecutiveFailures >= 2 ? (
                    <div
                        className={`flex items-start gap-2 rounded-md border px-3 py-2 text-xs ${
                            task.needsAttention ? "border-destructive/40 text-destructive" : "border-border text-muted-foreground"
                        }`}
                        role="status"
                        data-aipp-slot="schedule-detail-attention"
                    >
                        <AlertCircle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
                        <span className="break-words">
                            {task.consecutiveFailures >= 2 && task.enabled
                                ? `最近连续 ${task.consecutiveFailures} 次没有完成${task.lastRun?.reason ? `：${task.lastRun.reason}` : ""}`
                                : task.pausedReason}
                            {task.needsAttention && !task.enabled ? "。修好后打开「启用」即可恢复，暂停期间的时段不会补跑。" : ""}
                        </span>
                    </div>
                ) : null}

                <dl className="grid grid-cols-2 gap-x-6 gap-y-3 md:grid-cols-4" data-aipp-slot="schedule-detail-meta">
                    <MetaItem label="执行助手">
                        <span className="inline-flex items-center gap-1">
                            <User className="h-3.5 w-3.5 text-muted-foreground" />
                            {task.assistantName}
                        </span>
                    </MetaItem>
                    <MetaItem label="结果发到">
                        <span className="inline-flex items-center gap-1">
                            <Send className="h-3.5 w-3.5 text-muted-foreground" />
                            {task.deliverTargetText}
                        </span>
                    </MetaItem>
                    <MetaItem label="什么时候发">{deliverPolicyLabels[task.deliverPolicy]?.label ?? task.deliverPolicy}</MetaItem>
                    <MetaItem label="已运行">
                        {task.runCount} 次{endText ? `（${endText}）` : ""}
                    </MetaItem>
                </dl>

                <PromptBlock text={task.prompt} />
            </section>

            <div className="grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-[minmax(240px,2fr)_3fr]">
                <section className="flex min-h-60 flex-col overflow-hidden rounded-lg border border-border" data-aipp-slot="schedule-detail-runs">
                    <PanelHeader
                        title="运行记录"
                        meta={runs.length > 0 ? `${runs.length} 条` : undefined}
                        action={
                            <Button
                                variant="ghost"
                                size="icon"
                                className="size-7"
                                onClick={onRefreshRuns}
                                disabled={isRunsLoading}
                                title="刷新"
                                aria-label="刷新运行记录"
                            >
                                <RefreshCw className={`h-3.5 w-3.5 ${isRunsLoading ? "animate-spin" : ""}`} />
                            </Button>
                        }
                    />
                    <div className="min-h-0 flex-1 overflow-y-auto thin-scrollbar">
                        {isRunsLoading && runs.length === 0 ? (
                            <PanelMessage>加载记录…</PanelMessage>
                        ) : runs.length === 0 ? (
                            <PanelMessage>还没有运行过</PanelMessage>
                        ) : (
                            <ul className="divide-y divide-border">
                                {runs.map((run) => {
                                    const selected = run.runId === selectedRunId;
                                    const duration = formatRunDuration(run.startedTime, run.finishedTime);
                                    const detail = run.reason || run.summary;
                                    return (
                                        <li key={run.runId}>
                                            <button
                                                type="button"
                                                aria-current={selected ? "true" : undefined}
                                                onClick={() => onSelectRun(run.runId)}
                                                className={`block w-full px-3 py-2.5 text-left transition-colors ${
                                                    selected ? "bg-muted" : "hover:bg-muted/50"
                                                }`}
                                            >
                                                <div className="flex items-center gap-2 text-xs">
                                                    <RunStatusDot status={run.status} />
                                                    <span className={`font-medium ${isProblemStatus(run.status) ? "text-destructive" : ""}`}>
                                                        {run.statusText || runStatusLabel(run.status)}
                                                    </span>
                                                    <span className="text-muted-foreground">
                                                        {formatShortDateTime(run.scheduledFor ?? run.startedTime)}
                                                    </span>
                                                    {duration && <span className="text-muted-foreground">· {duration}</span>}
                                                </div>
                                                {detail && (
                                                    <p className="mt-1 line-clamp-2 break-words pl-4 text-[11px] leading-4 text-muted-foreground">{detail}</p>
                                                )}
                                            </button>
                                        </li>
                                    );
                                })}
                            </ul>
                        )}
                    </div>
                </section>

                <section className="flex min-h-60 flex-col overflow-hidden rounded-lg border border-border" data-aipp-slot="schedule-detail-logs">
                    <PanelHeader
                        title="运行过程"
                        meta={
                            selectedRun
                                ? [
                                      selectedRun.statusText || runStatusLabel(selectedRun.status),
                                      formatShortDateTime(selectedRun.startedTime),
                                      formatRunDuration(selectedRun.startedTime, selectedRun.finishedTime),
                                  ]
                                      .filter(Boolean)
                                      .join(" · ")
                                : undefined
                        }
                        action={isLogsLoading ? <RefreshCw className="h-3.5 w-3.5 animate-spin text-muted-foreground" /> : null}
                    />
                    <div className="min-h-0 flex-1 overflow-y-auto thin-scrollbar">
                        {!selectedRun ? (
                            <PanelMessage>选择左侧一条运行记录查看过程</PanelMessage>
                        ) : (
                            <div className="space-y-0">
                                {(selectedRun.reason || output || selectedRun.deliveryError) && (
                                    <div className="space-y-2 border-b border-border px-3 py-3" data-aipp-slot="schedule-run-summary">
                                        {selectedRun.reason && (
                                            <p className={`text-xs break-words ${isProblemStatus(selectedRun.status) ? "text-destructive" : "text-muted-foreground"}`}>
                                                {selectedRun.reason}
                                            </p>
                                        )}
                                        {output && (
                                            <div className="space-y-1">
                                                <div className="flex items-center gap-1 text-[11px] text-muted-foreground">
                                                    {selectedRun.status === "delivered" ? (
                                                        <>
                                                            <ArrowRight className="h-3 w-3" />
                                                            已发送到{task.deliverTargetText}
                                                        </>
                                                    ) : (
                                                        "产出（未发送）"
                                                    )}
                                                </div>
                                                <div className="max-h-56 overflow-y-auto whitespace-pre-wrap break-words rounded-md bg-muted/40 px-3 py-2 text-[13px] leading-6 thin-scrollbar">
                                                    {output}
                                                </div>
                                            </div>
                                        )}
                                    </div>
                                )}
                                {isLogsLoading && logs.length === 0 ? (
                                    <PanelMessage>加载过程…</PanelMessage>
                                ) : logs.length === 0 ? (
                                    <PanelMessage>这次运行没有过程记录</PanelMessage>
                                ) : (
                                    <ol className="divide-y divide-border/60">
                                        {logs.map((log) => (
                                            <LogLine key={log.id} log={log} onOpenToolLog={onOpenToolLog} />
                                        ))}
                                    </ol>
                                )}
                            </div>
                        )}
                    </div>
                </section>
            </div>
        </div>
    );
}

export default memo(ScheduleTaskDetail);
