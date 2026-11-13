/** 与后端 `scheduler::service::RunView` 保持一致。 */
export type ScheduledRunStatus =
    | "running"
    | "delivered"
    | "quiet"
    | "failed"
    | "skipped"
    | "missed"
    | "cancelled"
    /** 旧版本的成功记录 */
    | "success";

export interface ScheduledRun {
    runId: string;
    status: ScheduledRunStatus;
    statusText: string;
    reason?: string | null;
    summary?: string | null;
    scheduledFor?: string | null;
    startedTime: string;
    finishedTime?: string | null;
    conversationId?: number | null;
    finalMessageId?: number | null;
    deliveredMessageId?: number | null;
    deliveryConversationId?: number | null;
    deliveryError?: string | null;
    /** 本进程持有这次运行的停止令牌。 */
    localRunning: boolean;
}

export type DeliverPolicy = "always" | "if_needed" | "never";
export type DeliverTo = "owner_chat" | "conversation" | "run_conversation";
export type MissedPolicy = "run_once" | "skip";

/** 与后端 `scheduler::service::TaskView` 保持一致。 */
export interface ScheduledTask {
    id: number;
    name: string;
    enabled: boolean;
    schedule: string;
    scheduleText: string;
    upcoming: string[];
    upcomingText: string[];
    nextRunAt?: string | null;
    lastRunAt?: string | null;
    assistantId: number;
    assistantName: string;
    prompt: string;
    deliverPolicy: DeliverPolicy;
    deliverTo: DeliverTo;
    deliverConversationId?: number | null;
    deliverTargetText: string;
    createdInConversationId?: number | null;
    butlerMode: boolean;
    endsAt?: string | null;
    maxRuns?: number | null;
    runCount: number;
    missedPolicy: MissedPolicy;
    consecutiveFailures: number;
    pausedReason?: string | null;
    createdTime: string;
    updatedTime: string;
    lastRun?: ScheduledRun | null;
    needsAttention: boolean;
}

export interface SchedulePreview {
    schedule: string;
    scheduleText: string;
    upcoming: string[];
    upcomingText: string[];
}

/** 创建任务的输入（`create_scheduled_task`）。 */
export interface ScheduledTaskInput {
    name: string;
    schedule: string;
    prompt: string;
    assistantId: number;
    deliverPolicy?: DeliverPolicy;
    deliverTo?: DeliverTo;
    deliverConversationId?: number | null;
    endsAt?: string | null;
    maxRuns?: number | null;
    missedPolicy?: MissedPolicy;
    enabled?: boolean;
}

/** 修改任务（`update_scheduled_task`）：只改出现的字段；endsAt 传空字符串、maxRuns 传 0 表示清除。 */
export type ScheduledTaskPatch = Partial<ScheduledTaskInput>;

export interface ScheduledTaskLog {
    id: number;
    taskId: number;
    runId: string;
    messageType: string;
    content: string;
    createdTime: string;
}

export interface ScheduledTaskRunEvent {
    taskId: number;
    runId: string;
    status: ScheduledRunStatus;
}

export interface ScheduledTaskToolLogPayload {
    callId?: string;
    serverName?: string;
    toolName?: string;
    success?: boolean;
    parameters?: unknown;
    result?: unknown;
}
