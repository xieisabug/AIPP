import ButlerCheckPolicy from "./ButlerCheckPolicy";
import { memo, useState } from "react";
import { Bell, Calendar, Clock, MoreHorizontal, Pencil, Play, RefreshCw, Square, Trash2, User } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Switch } from "@/components/ui/switch";
import {
    DropdownMenu,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import type { ScheduledTask, ScheduledTaskLog, ScheduledTaskRun, ScheduledTaskRunStatus } from "./scheduleTypes";
import {
    formatClockTime,
    formatRunDuration,
    formatShortDateTime,
    logTypeLabels,
    parseToolLogPayload,
    runStatusLabel,
} from "./scheduleFormat";

export interface ScheduleTaskDetailProps {
    task: ScheduledTask;
    scheduleDescription: string;
    assistantName: string;
    runs: ScheduledTaskRun[];
    selectedRunId: string | null;
    isRunsLoading: boolean;
    logs: ScheduledTaskLog[];
    isLogsLoading: boolean;
    runningRun: ScheduledTaskRun | null;
    isRunning: boolean;
    isStopping: boolean;
    onSelectRun: (runId: string) => void;
    onRefreshRuns: () => void;
    onToggleEnabled: (enabled: boolean) => void;
    onEdit: () => void;
    onRunNow: () => void;
    onStop: () => void;
    onDelete: () => void;
    onOpenToolLog: (log: ScheduledTaskLog) => void;
}

/** 状态圆点：只用灰阶区分，失败用 destructive 作为唯一强调色。 */
function RunStatusDot({ status }: { status: ScheduledTaskRunStatus | string }) {
    const base = "inline-block size-2 shrink-0 rounded-full";
    if (status === "running") return <span className={`${base} bg-foreground animate-pulse`} aria-hidden="true" />;
    if (status === "failed") return <span className={`${base} bg-destructive`} aria-hidden="true" />;
    if (status === "missed") return <span className={`${base} border border-dashed border-muted-foreground`} aria-hidden="true" />;
    return <span className={`${base} border border-foreground/60`} aria-hidden="true" />;
}

const PROMPT_COLLAPSE_CHARS = 320;
const PROMPT_COLLAPSE_LINES = 7;

function PromptBlock({ title, text, placeholder }: { title: string; text: string; placeholder: string }) {
    const [expanded, setExpanded] = useState(false);
    const value = text.trim();
    const collapsible = value.length > PROMPT_COLLAPSE_CHARS || value.split("\n").length > PROMPT_COLLAPSE_LINES;
    return (
        <div className="min-w-0 space-y-1" data-aipp-slot="schedule-detail-prompt">
            <div className="flex items-center justify-between gap-2">
                <span className="text-xs font-medium text-muted-foreground">{title}</span>
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
                className={`whitespace-pre-wrap break-words min-h-24 rounded-md bg-muted/40 px-3.5 py-3 text-[13px] leading-6 ${
                    value ? "text-foreground" : "text-muted-foreground"
                } ${collapsible && !expanded ? "line-clamp-7" : "max-h-[28rem] overflow-y-auto thin-scrollbar"}`}
            >
                {value || placeholder}
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

function ScheduleTaskDetail({
    task,
    scheduleDescription,
    assistantName,
    runs,
    selectedRunId,
    isRunsLoading,
    logs,
    isLogsLoading,
    runningRun,
    isRunning,
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
    const selectedRunDuration = selectedRun ? formatRunDuration(selectedRun.startedTime, selectedRun.finishedTime) : null;

    return (
        // 最小高度兜底：窗口很矮时退回外层整页滚动，而不是把两栏压扁
        <div className="flex h-full min-h-[760px] flex-col gap-4" data-aipp-slot="schedule-task-detail">
            {task.butlerConversationId && <ButlerCheckPolicy taskId={task.id} />}
            <section className="shrink-0 space-y-5 rounded-lg border border-border p-5" data-aipp-slot="schedule-detail-overview">
                <div className="flex flex-wrap items-start justify-between gap-3">
                    <div className="min-w-0 flex-1">
                        <div className="flex min-w-0 items-center gap-2">
                            <h2 className="truncate text-lg font-semibold">{task.name}</h2>
                            {task.butlerConversationId ? (
                                <Badge variant="outline" className="shrink-0 text-[11px] font-normal text-muted-foreground">
                                    管家
                                </Badge>
                            ) : null}
                            {!task.isEnabled && (
                                <Badge variant="secondary" className="shrink-0 text-[11px] font-normal">
                                    已停用
                                </Badge>
                            )}
                        </div>
                        <p className="mt-1.5 text-sm text-muted-foreground">{scheduleDescription}</p>
                    </div>
                    <div className="flex shrink-0 items-center gap-2">
                        <label className="flex items-center gap-2 text-xs text-muted-foreground">
                            启用
                            <Switch disabled={!!task.butlerConversationId} checked={task.isEnabled} onCheckedChange={onToggleEnabled} aria-label="启用任务" />
                        </label>
                        {runningRun ? (
                            <Button variant="outline" size="sm" onClick={onStop} disabled={isStopping}>
                                <Square className="h-3.5 w-3.5" />
                                {isStopping ? "停止中…" : "停止"}
                            </Button>
                        ) : (
                            <Button variant="outline" size="sm" onClick={onRunNow} disabled={isRunning}>
                                <Play className="h-3.5 w-3.5" />
                                {isRunning ? "执行中…" : "立即执行"}
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

                <dl className="flex flex-wrap gap-x-8 gap-y-2 text-sm" data-aipp-slot="schedule-detail-meta">
                    <div className="flex items-center gap-1.5">
                        <Calendar className="h-3.5 w-3.5 text-muted-foreground" />
                        <dt className="text-muted-foreground">下次</dt>
                        <dd>{task.nextRunAt ? formatShortDateTime(task.nextRunAt) : "未安排"}</dd>
                    </div>
                    <div className="flex items-center gap-1.5">
                        <Clock className="h-3.5 w-3.5 text-muted-foreground" />
                        <dt className="text-muted-foreground">上次</dt>
                        <dd>{task.lastRunAt ? formatShortDateTime(task.lastRunAt) : "未执行"}</dd>
                    </div>
                    <div className="flex items-center gap-1.5">
                        <User className="h-3.5 w-3.5 text-muted-foreground" />
                        <dt className="text-muted-foreground">助手</dt>
                        <dd>{assistantName || "未选择"}</dd>
                    </div>
                </dl>

                <div className="grid gap-4 lg:grid-cols-2">
                    <PromptBlock title="任务指令" text={task.taskPrompt} placeholder="未设置" />
                    <PromptBlock title="通知判定规则" text={task.notifyPrompt} placeholder="默认规则：有重要信息时通知" />
                </div>
            </section>

            <div className="grid min-h-0 flex-1 grid-cols-1 gap-4 lg:grid-cols-[minmax(240px,2fr)_3fr]">
                <section
                    className="flex min-h-60 flex-col overflow-hidden rounded-lg border border-border"
                    data-aipp-slot="schedule-detail-runs"
                >
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
                            <PanelMessage>暂无运行记录</PanelMessage>
                        ) : (
                            <ul className="divide-y divide-border">
                                {runs.map((run) => {
                                    const selected = run.runId === selectedRunId;
                                    const duration = formatRunDuration(run.startedTime, run.finishedTime);
                                    const detail = run.errorMessage || run.summary;
                                    return (
                                        <li key={run.id}>
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
                                                    <span className={`font-medium ${run.status === "failed" ? "text-destructive" : ""}`}>
                                                        {runStatusLabel(run.status)}
                                                    </span>
                                                    <span className="text-muted-foreground">{formatShortDateTime(run.startedTime)}</span>
                                                    {duration && <span className="text-muted-foreground">· {duration}</span>}
                                                    {run.notify && (
                                                        <span className="ml-auto inline-flex items-center gap-1 text-muted-foreground" title="已通知">
                                                            <Bell className="h-3 w-3" />
                                                            已通知
                                                        </span>
                                                    )}
                                                </div>
                                                {detail && (
                                                    <p className="mt-1 line-clamp-2 break-words pl-4 text-[11px] leading-4 text-muted-foreground">
                                                        {detail}
                                                    </p>
                                                )}
                                            </button>
                                        </li>
                                    );
                                })}
                            </ul>
                        )}
                    </div>
                </section>

                <section
                    className="flex min-h-60 flex-col overflow-hidden rounded-lg border border-border"
                    data-aipp-slot="schedule-detail-logs"
                >
                    <PanelHeader
                        title="运行日志"
                        meta={
                            selectedRun
                                ? [
                                      runStatusLabel(selectedRun.status),
                                      formatShortDateTime(selectedRun.startedTime),
                                      selectedRunDuration,
                                      selectedRun.runId.slice(0, 8),
                                  ]
                                      .filter(Boolean)
                                      .join(" · ")
                                : undefined
                        }
                        action={isLogsLoading ? <RefreshCw className="h-3.5 w-3.5 animate-spin text-muted-foreground" /> : null}
                    />
                    <div className="min-h-0 flex-1 overflow-y-auto thin-scrollbar">
                        {!selectedRun ? (
                            <PanelMessage>选择左侧一条运行记录查看日志</PanelMessage>
                        ) : isLogsLoading && logs.length === 0 ? (
                            <PanelMessage>加载日志…</PanelMessage>
                        ) : logs.length === 0 ? (
                            <PanelMessage>暂无日志</PanelMessage>
                        ) : (
                            <ol className="divide-y divide-border/60">
                                {logs.map((log) => {
                                    const isError = log.messageType === "error";
                                    const toolPayload = parseToolLogPayload(log);
                                    return (
                                        <li key={log.id} className="flex gap-3 px-3 py-2 text-xs">
                                            <span className="w-14 shrink-0 font-mono text-[11px] leading-5 text-muted-foreground">
                                                {formatClockTime(log.createdTime)}
                                            </span>
                                            <div className="min-w-0 flex-1 space-y-0.5">
                                                <span
                                                    className={`text-[11px] font-medium leading-5 ${
                                                        isError ? "text-destructive" : "text-muted-foreground"
                                                    }`}
                                                >
                                                    {logTypeLabels[log.messageType] ?? log.messageType}
                                                </span>
                                                {toolPayload ? (
                                                    <div className="flex flex-wrap items-center gap-x-2 leading-5">
                                                        <span>
                                                            {log.messageType === "tool_result"
                                                                ? toolPayload.success
                                                                    ? "执行成功"
                                                                    : "执行失败"
                                                                : "准备执行"}
                                                            ：{toolPayload.serverName ?? "-"} / {toolPayload.toolName ?? "-"}
                                                        </span>
                                                        <button
                                                            type="button"
                                                            className="text-muted-foreground underline-offset-2 hover:text-foreground hover:underline"
                                                            onClick={() => onOpenToolLog(log)}
                                                        >
                                                            参数 / 返回
                                                        </button>
                                                    </div>
                                                ) : (
                                                    <div
                                                        className={`whitespace-pre-wrap break-words leading-5 ${
                                                            isError ? "text-destructive" : ""
                                                        }`}
                                                    >
                                                        {log.content}
                                                    </div>
                                                )}
                                            </div>
                                        </li>
                                    );
                                })}
                            </ol>
                        )}
                    </div>
                </section>
            </div>
        </div>
    );
}

export default memo(ScheduleTaskDetail);
