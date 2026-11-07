export interface ScheduledTask {
    id: number;
    name: string;
    isEnabled: boolean;
    scheduleType: "once" | "interval";
    intervalValue?: number | null;
    intervalUnit?: string | null;
    startTime?: string | null;
    weekDays?: number[] | null;
    monthDays?: number[] | null;
    runAt?: string | null;
    nextRunAt?: string | null;
    lastRunAt?: string | null;
    assistantId: number;
    taskPrompt: string;
    notifyPrompt: string;
    butlerConversationId?: number | null;
    createdTime: string;
    updatedTime: string;
}

export interface ScheduledTaskLog {
    id: number;
    taskId: number;
    runId: string;
    messageType: string;
    content: string;
    createdTime: string;
}

/** 与数据库 scheduled_task_run.status 的 CHECK 约束保持一致。 */
export type ScheduledTaskRunStatus = "running" | "success" | "failed" | "missed";

export interface ScheduledTaskRun {
    id: number;
    taskId: number;
    runId: string;
    status: ScheduledTaskRunStatus;
    notify: boolean;
    summary?: string | null;
    errorMessage?: string | null;
    startedTime: string;
    finishedTime?: string | null;
    butlerFollowupStatus?: string | null;
}

export interface ScheduledTaskRunUpdateEvent {
    runId: string;
    status: ScheduledTaskRunStatus;
    notify: boolean;
    summary?: string | null;
    errorMessage?: string | null;
    finishedTime?: string | null;
}

export interface ScheduledTaskToolLogPayload {
    callId?: string;
    serverName?: string;
    toolName?: string;
    success?: boolean;
    parameters?: unknown;
    result?: unknown;
}
