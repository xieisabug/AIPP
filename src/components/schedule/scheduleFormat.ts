import { format, isSameYear } from "date-fns";
import type { DeliverPolicy, ScheduledRunStatus, ScheduledTask, ScheduledTaskLog, ScheduledTaskToolLogPayload } from "./scheduleTypes";

export const logTypeLabels: Record<string, string> = {
    start: "开始",
    task_prompt: "任务指令",
    assistant: "助手信息",
    tool_round: "工具轮次",
    tool_call: "工具调用",
    admitted: "放行",
    review_held: "审核未通过",
    butler_decision: "总管家裁决",
    needs_user: "需要用户处理",
    tool_result: "工具结果",
    loop_done: "执行完成",
    llm_retry: "重试",
    timeout: "超时",
    max_rounds: "达到轮次上限",
    cancel_request: "停止请求",
    cancel: "已停止",
    response: "产出",
    delivered: "已发送",
    delivery_error: "发送失败",
    skipped: "跳过",
    interrupted: "中断",
    result_saved: "结果已保存",
    cleanup_error: "清理失败",
    error: "错误",
};

const runStatusLabels: Record<ScheduledRunStatus, string> = {
    running: "运行中",
    delivered: "已发送",
    quiet: "无新内容",
    failed: "失败",
    skipped: "跳过",
    missed: "错过",
    cancelled: "已停止",
    success: "完成",
};

export function runStatusLabel(status: string): string {
    return runStatusLabels[status as ScheduledRunStatus] ?? status;
}

/** 需要强调的运行状态（只有失败类用 destructive）。 */
export function isProblemStatus(status: string): boolean {
    return status === "failed" || status === "skipped";
}

export const deliverPolicyLabels: Record<DeliverPolicy, { label: string; description: string }> = {
    always: { label: "每次都发", description: "适合提醒：每次运行都把结果发给你" },
    if_needed: { label: "有新内容才发", description: "适合检查、监控：没有值得说的内容时不打扰你" },
    never: { label: "只记录", description: "结果只留在运行记录里，不发送" },
};

export type TaskGroup = "attention" | "active" | "paused" | "finished";

export const taskGroupLabels: Record<TaskGroup, string> = {
    attention: "需要处理",
    active: "已启用",
    paused: "已暂停",
    finished: "已结束",
};

/** 列表分组：需要处理 → 已启用 → 已暂停 → 已结束。 */
export function taskGroup(task: Pick<ScheduledTask, "enabled" | "needsAttention" | "pausedReason">): TaskGroup {
    if (task.needsAttention) return "attention";
    if (task.enabled) return "active";
    return task.pausedReason === "已完成" ? "finished" : "paused";
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
