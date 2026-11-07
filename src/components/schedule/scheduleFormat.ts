import { format, isSameYear } from "date-fns";
import type { ScheduledTaskLog, ScheduledTaskRunStatus, ScheduledTaskToolLogPayload } from "./scheduleTypes";

export const logTypeLabels: Record<string, string> = {
    start: "开始",
    task_prompt: "任务指令",
    assistant: "助手信息",
    tool_round: "工具轮次",
    tool_call: "工具调用",
    tool_result: "工具结果",
    loop_done: "执行完成",
    llm_retry: "重试",
    timeout: "超时",
    max_rounds: "达到轮次上限",
    cancel_request: "停止请求",
    cancel: "已停止",
    response: "任务输出",
    notify_raw: "通知判定原文",
    notify_result: "通知判定结果",
    notify: "系统通知",
    cleanup: "清理",
    error: "错误",
};

const runStatusLabels: Record<ScheduledTaskRunStatus, string> = {
    running: "运行中",
    success: "完成",
    failed: "失败",
    missed: "错过",
};

export function runStatusLabel(status: string): string {
    return runStatusLabels[status as ScheduledTaskRunStatus] ?? status;
}

const parseDate = (value?: string | null): Date | null => {
    if (!value) return null;
    const date = new Date(value);
    return Number.isNaN(date.getTime()) ? null : date;
};

/** 列表里的时间：同年省略年份，跨年才显示，避免每行都是一长串。 */
export function formatShortDateTime(value?: string | null, now: Date = new Date()): string {
    const date = parseDate(value);
    if (!date) return "";
    return format(date, isSameYear(date, now) ? "MM-dd HH:mm" : "yyyy-MM-dd HH:mm");
}

export function formatClockTime(value?: string | null): string {
    const date = parseDate(value);
    return date ? format(date, "HH:mm:ss") : "";
}

/** 已结束运行的耗时；未结束或时间缺失时返回 null。 */
export function formatRunDuration(startedTime?: string | null, finishedTime?: string | null): string | null {
    const start = parseDate(startedTime);
    const end = parseDate(finishedTime);
    if (!start || !end) return null;
    const totalSeconds = Math.max(0, Math.round((end.getTime() - start.getTime()) / 1000));
    if (totalSeconds < 60) return `${totalSeconds} 秒`;
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;
    if (hours > 0) return minutes > 0 ? `${hours} 小时 ${minutes} 分` : `${hours} 小时`;
    return seconds > 0 ? `${minutes} 分 ${seconds} 秒` : `${minutes} 分`;
}

export function parseToolLogPayload(log: ScheduledTaskLog): ScheduledTaskToolLogPayload | null {
    if (log.messageType !== "tool_call" && log.messageType !== "tool_result") {
        return null;
    }
    try {
        const parsed = JSON.parse(log.content) as ScheduledTaskToolLogPayload;
        return parsed && typeof parsed === "object" ? parsed : null;
    } catch {
        return null;
    }
}
