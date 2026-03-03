import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { format } from "date-fns";
import { useTheme } from "@/hooks/useTheme";
import { useToast } from "@/hooks/use-toast";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Label } from "@/components/ui/label";
import { Checkbox } from "@/components/ui/checkbox";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Calendar as DateCalendar } from "@/components/ui/calendar";
import { ConfigPageLayout, SidebarList, ListItemButton, EmptyState } from "@/components/common";
import ConfirmDialog from "@/components/ConfirmDialog";
import { AssistantListItem } from "@/data/Assistant";
import { useAssistantListListener } from "@/hooks/useAssistantListListener";
import { Calendar, Clock, Plus, RefreshCw, Trash2, Pencil, Play, Bell, HelpCircle, Square } from "lucide-react";

interface ScheduledTask {
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
    createdTime: string;
    updatedTime: string;
}

interface ScheduledTaskLog {
    id: number;
    taskId: number;
    runId: string;
    messageType: string;
    content: string;
    createdTime: string;
}

interface ScheduledTaskRun {
    id: number;
    taskId: number;
    runId: string;
    status: "running" | "success" | "failed";
    notify: boolean;
    summary?: string | null;
    errorMessage?: string | null;
    startedTime: string;
    finishedTime?: string | null;
}

interface ScheduledTaskToolLogPayload {
    callId?: string;
    serverName?: string;
    toolName?: string;
    success?: boolean;
    parameters?: unknown;
    result?: unknown;
}

interface ScheduledTaskFormValues {
    name: string;
    is_enabled: boolean;
    schedule_type: "once" | "interval";
    run_at: string;
    interval_value: string;
    interval_unit: string;
    start_time: string;
    week_days: number[];
    month_days: number[];
    assistant_id: string;
    task_prompt: string;
    notify_prompt: string;
}

interface ScheduledTaskSavePayload {
    name: string;
    isEnabled: boolean;
    scheduleType: "once" | "interval";
    intervalValue: number | null;
    intervalUnit: string | null;
    startTime: string | null;
    weekDays: number[] | null;
    monthDays: number[] | null;
    runAt: string | null;
    assistantId: number;
    taskPrompt: string;
    notifyPrompt: string;
}

const intervalUnitLabels: Record<string, string> = {
    minute: "分钟",
    hour: "小时",
    day: "天",
    week: "周",
    month: "月",
};

const weekDayLabels: Record<number, string> = {
    0: "周日",
    1: "周一",
    2: "周二",
    3: "周三",
    4: "周四",
    5: "周五",
    6: "周六",
};

const logTypeLabels: Record<string, string> = {
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

const toLocalDatetimeInput = (value?: string | null) => {
    if (!value) return "";
    const date = new Date(value);
    return format(date, "yyyy-MM-dd'T'HH:mm");
};

const DEFAULT_ONCE_TIME = "09:00";

const formatToolLogValue = (value: unknown): string => {
    if (typeof value === "string") {
        return value;
    }
    try {
        return JSON.stringify(value, null, 2);
    } catch {
        return String(value ?? "");
    }
};

const pad2 = (value: number) => String(value).padStart(2, "0");

const parseRunAtParts = (value: string): { date: string; time: string } | null => {
    const raw = value.trim();
    if (!raw) return null;
    const normalized = raw
        .replace(/[年月]/g, "-")
        .replace(/日/g, "")
        .replace(/\//g, "-")
        .replace(/T/g, " ")
        .replace(/\s+/g, " ")
        .trim();
    const match = normalized.match(/^(\d{4})-(\d{1,2})-(\d{1,2})\s+(\d{1,2}):(\d{1,2})/);
    if (!match) {
        return null;
    }
    const [, y, m, d, hh, mm] = match;
    const year = Number(y);
    const month = Number(m);
    const day = Number(d);
    const hour = Number(hh);
    const minute = Number(mm);
    if (
        month < 1 || month > 12 ||
        day < 1 || day > 31 ||
        hour < 0 || hour > 23 ||
        minute < 0 || minute > 59
    ) {
        return null;
    }
    return {
        date: `${year}-${pad2(month)}-${pad2(day)}`,
        time: `${pad2(hour)}:${pad2(minute)}`,
    };
};

const parseDatePartToLocalDate = (datePart: string): Date | undefined => {
    const match = datePart.match(/^(\d{4})-(\d{2})-(\d{2})$/);
    if (!match) {
        return undefined;
    }
    const [, y, m, d] = match;
    const year = Number(y);
    const month = Number(m);
    const day = Number(d);
    const date = new Date(year, month - 1, day, 0, 0, 0);
    if (
        Number.isNaN(date.getTime()) ||
        date.getFullYear() !== year ||
        date.getMonth() !== month - 1 ||
        date.getDate() !== day
    ) {
        return undefined;
    }
    return date;
};

const composeRunAtValue = (datePart: string, timePart: string): string => {
    const date = datePart.trim();
    const time = timePart.trim();
    if (!date || !time) {
        return "";
    }
    return `${date}T${time}`;
};

const toServerDatetime = (value: string): string | null => {
    const raw = value.trim();
    if (!raw) return null;

    const normalized = raw
        .replace(/[年月]/g, "-")
        .replace(/日/g, "")
        .replace(/\//g, "-")
        .replace(/T/g, " ")
        .replace(/\s+/g, " ");
    const match = normalized.match(
        /^(\d{4})-(\d{1,2})-(\d{1,2})\s+(\d{1,2}):(\d{1,2})(?::(\d{1,2}))?(?:\.\d+)?(?:\s*(?:Z|[+-]\d{2}:?\d{2}))?$/i
    );
    if (!match) {
        const fallbackDate = new Date(raw);
        if (!Number.isNaN(fallbackDate.getTime())) {
            return format(fallbackDate, "yyyy-MM-dd HH:mm:ss");
        }
        const normalizedFallbackDate = new Date(normalized);
        if (!Number.isNaN(normalizedFallbackDate.getTime())) {
            return format(normalizedFallbackDate, "yyyy-MM-dd HH:mm:ss");
        }
        return null;
    }

    const [, y, m, d, hh, mm, ss = "0"] = match;
    const year = Number(y);
    const month = Number(m);
    const day = Number(d);
    const hour = Number(hh);
    const minute = Number(mm);
    const second = Number(ss);
    if (
        month < 1 || month > 12 ||
        day < 1 || day > 31 ||
        hour < 0 || hour > 23 ||
        minute < 0 || minute > 59 ||
        second < 0 || second > 59
    ) {
        return null;
    }

    const date = new Date(year, month - 1, day, hour, minute, second);
    if (
        Number.isNaN(date.getTime()) ||
        date.getFullYear() !== year ||
        date.getMonth() !== month - 1 ||
        date.getDate() !== day ||
        date.getHours() !== hour ||
        date.getMinutes() !== minute ||
        date.getSeconds() !== second
    ) {
        return null;
    }

    return format(date, "yyyy-MM-dd HH:mm:ss");
};

const getErrorMessage = (error: unknown): string => {
    if (typeof error === "string") {
        return error;
    }
    if (error instanceof Error) {
        return error.message || String(error);
    }
    if (error && typeof error === "object") {
        const candidate = error as Record<string, unknown>;
        const messageCandidates = [candidate.message, candidate.error, candidate.reason, candidate.details];
        const text = messageCandidates.find((item) => typeof item === "string" && item.trim()) as string | undefined;
        if (text) {
            return text;
        }
        try {
            return JSON.stringify(candidate);
        } catch {
            return String(error);
        }
    }
    return String(error);
};

const humanizeSaveError = (message: string): string => {
    if (!message.trim()) {
        return "保存失败：未知错误，请查看控制台日志。";
    }
    if (message.includes("无法解析时间") || message.includes("一次性任务需要设置执行时间")) {
        return "保存失败：执行时间格式无效，请重新选择“指定时间执行一次”的时间。";
    }
    if (message.includes("只能选择普通对话助手")) {
        return "保存失败：当前助手类型不支持定时任务，请选择普通对话助手。";
    }
    if (message.includes("任务不存在")) {
        return "保存失败：任务不存在，可能已被删除，请刷新后重试。";
    }
    return message;
};

export default function ScheduleWindow() {
    useTheme("schedule");
    const { toast } = useToast();

    const [tasks, setTasks] = useState<ScheduledTask[]>([]);
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [activeTaskId, setActiveTaskId] = useState<number | null>(null);
    const [editingTaskId, setEditingTaskId] = useState<number | null>(null);
    const [isDialogOpen, setIsDialogOpen] = useState(false);
    const [isHelpDialogOpen, setIsHelpDialogOpen] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [isRunning, setIsRunning] = useState(false);
    const [isStopping, setIsStopping] = useState(false);
    const [isRefreshing, setIsRefreshing] = useState(false);
    const [runEntries, setRunEntries] = useState<ScheduledTaskRun[]>([]);
    const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
    const [logEntries, setLogEntries] = useState<ScheduledTaskLog[]>([]);
    const [isLogLoading, setIsLogLoading] = useState(false);
    const [selectedToolLog, setSelectedToolLog] = useState<ScheduledTaskLog | null>(null);
    const [deleteTarget, setDeleteTarget] = useState<ScheduledTask | null>(null);
    const autoRefreshBusyRef = useRef(false);
    const [formValues, setFormValues] = useState<ScheduledTaskFormValues>({
        name: "",
        is_enabled: true,
        schedule_type: "once",
        run_at: "",
        interval_value: "1",
        interval_unit: "hour",
        start_time: "09:00",
        week_days: [1],
        month_days: [1],
        assistant_id: "",
        task_prompt: "",
        notify_prompt: "",
    });

    const selectedTask = useMemo(
        () => tasks.find((task) => task.id === activeTaskId) ?? null,
        [tasks, activeTaskId]
    );

    const assistantOptions = useMemo(
        () => assistants.filter((assistant) => assistant.assistant_type === 0),
        [assistants]
    );

    const onceRunAtParts = useMemo(() => parseRunAtParts(formValues.run_at), [formValues.run_at]);
    const onceDatePart = onceRunAtParts?.date ?? "";
    const onceTimePart = onceRunAtParts?.time ?? "";
    const onceSelectedDate = useMemo(() => parseDatePartToLocalDate(onceDatePart), [onceDatePart]);

    const handleOnceDateChange = useCallback((date?: Date) => {
        setFormValues((prev) => {
            if (!date) {
                return { ...prev, run_at: "" };
            }
            const currentParts = parseRunAtParts(prev.run_at);
            const nextDate = format(date, "yyyy-MM-dd");
            return { ...prev, run_at: composeRunAtValue(nextDate, currentParts?.time ?? DEFAULT_ONCE_TIME) };
        });
    }, []);

    const handleOnceTimeChange = useCallback((timeValue: string) => {
        setFormValues((prev) => {
            const currentParts = parseRunAtParts(prev.run_at);
            if (!currentParts?.date) {
                return { ...prev, run_at: "" };
            }
            return { ...prev, run_at: composeRunAtValue(currentParts.date, timeValue) };
        });
    }, []);

    const loadTasks = useCallback(async () => {
        try {
            setIsLoading(true);
            const result = await invoke<ScheduledTask[]>("list_scheduled_tasks");
            setTasks(result);
            if (result.length > 0 && !activeTaskId) {
                setActiveTaskId(result[0].id);
            } else if (result.length === 0) {
                setActiveTaskId(null);
            }
        } catch (error) {
            toast({
                title: "加载失败",
                description: error as string,
                variant: "destructive",
            });
        } finally {
            setIsLoading(false);
        }
    }, [activeTaskId, toast]);

    const loadAssistants = useCallback(async () => {
        try {
            const result = await invoke<AssistantListItem[]>("get_assistants");
            setAssistants(result);
            if (!formValues.assistant_id && result.length > 0) {
                const first = result.find((assistant) => assistant.assistant_type === 0);
                if (first) {
                    setFormValues((prev) => ({ ...prev, assistant_id: first.id.toString() }));
                }
            }
        } catch (error) {
            console.error("加载助手失败:", error);
        }
    }, [formValues.assistant_id]);

    useAssistantListListener({
        onAssistantListChanged: setAssistants,
    });

    useEffect(() => {
        loadTasks();
        loadAssistants();
    }, [loadTasks, loadAssistants]);

    const loadRuns = useCallback(
        async (taskId?: number | null, options?: { silent?: boolean }) => {
            if (!taskId) {
                setRunEntries([]);
                setSelectedRunId(null);
                return;
            }
            const silent = options?.silent ?? false;
            if (!silent) {
                setIsLogLoading(true);
            }
            try {
                const result = await invoke<{ runs: ScheduledTaskRun[] }>("list_scheduled_task_runs", {
                    taskId,
                    limit: 50,
                });
                setRunEntries(result.runs);
                setSelectedRunId((prev) => {
                    if (prev && result.runs.some((run) => run.runId === prev)) {
                        return prev;
                    }
                    return result.runs[0]?.runId ?? null;
                });
            } catch (error) {
                console.error("加载定时任务运行记录失败:", error);
                setRunEntries([]);
                setSelectedRunId(null);
            } finally {
                if (!silent) {
                    setIsLogLoading(false);
                }
            }
        },
        []
    );

    const loadLogs = useCallback(
        async (taskId?: number | null, runId?: string | null, options?: { silent?: boolean }) => {
            if (!taskId || !runId) {
                setLogEntries([]);
                return;
            }
            const silent = options?.silent ?? false;
            if (!silent) {
                setIsLogLoading(true);
            }
            try {
                const result = await invoke<{ logs: ScheduledTaskLog[] }>("list_scheduled_task_logs", {
                    taskId,
                    runId,
                    limit: 200,
                });
                setLogEntries(result.logs);
            } catch (error) {
                console.error("加载定时任务日志失败:", error);
                setLogEntries([]);
            } finally {
                if (!silent) {
                    setIsLogLoading(false);
                }
            }
        },
        []
    );

    useEffect(() => {
        loadRuns(selectedTask?.id ?? null);
    }, [loadRuns, selectedTask?.id]);

    useEffect(() => {
        loadLogs(selectedTask?.id ?? null, selectedRunId);
    }, [loadLogs, selectedTask?.id, selectedRunId]);

    useEffect(() => {
        if (!selectedTask?.id) {
            return;
        }

        const refreshNow = async () => {
            if (autoRefreshBusyRef.current) {
                return;
            }
            autoRefreshBusyRef.current = true;
            try {
                await loadRuns(selectedTask.id, { silent: true });
                const runId = selectedRunId ?? runEntries[0]?.runId ?? null;
                if (runId) {
                    await loadLogs(selectedTask.id, runId, { silent: true });
                }
            } finally {
                autoRefreshBusyRef.current = false;
            }
        };

        const timer = window.setInterval(() => {
            void refreshNow();
        }, 2000);

        return () => {
            window.clearInterval(timer);
        };
    }, [loadLogs, loadRuns, runEntries, selectedRunId, selectedTask?.id]);

    const handleRefresh = useCallback(async () => {
        setIsRefreshing(true);
        await loadTasks();
        setIsRefreshing(false);
    }, [loadTasks]);

    const openCreateDialog = useCallback(() => {
        setFormValues({
            name: "",
            is_enabled: true,
            schedule_type: "once",
            run_at: "",
            interval_value: "1",
            interval_unit: "hour",
            start_time: "09:00",
            week_days: [1],
            month_days: [1],
            assistant_id: assistantOptions[0]?.id.toString() ?? "",
            task_prompt: "",
            notify_prompt: "",
        });
        setEditingTaskId(null);
        setIsDialogOpen(true);
    }, [assistantOptions]);

    const openEditDialog = useCallback(
        (task: ScheduledTask) => {
            setFormValues({
                name: task.name,
                is_enabled: task.isEnabled,
                schedule_type: task.scheduleType,
                run_at: toLocalDatetimeInput(task.runAt),
                interval_value: task.intervalValue ? task.intervalValue.toString() : "1",
                interval_unit: task.intervalUnit ?? "hour",
                start_time: task.startTime ?? "09:00",
                week_days: task.weekDays ?? [1],
                month_days: task.monthDays ?? [1],
                assistant_id: task.assistantId.toString(),
                task_prompt: task.taskPrompt,
                notify_prompt: task.notifyPrompt || "",
            });
            setEditingTaskId(task.id);
            setActiveTaskId(task.id);
            setIsDialogOpen(true);
        },
        []
    );

    const handleSave = useCallback(async () => {
        let payload: ScheduledTaskSavePayload | null = null;
        setIsSaving(true);
        try {
            const needsStartTime = ["day", "week", "month"].includes(formValues.interval_unit);
            const onceRunAtRaw = formValues.schedule_type === "once" ? formValues.run_at.trim() : "";
            payload = {
                name: formValues.name.trim(),
                isEnabled: formValues.is_enabled,
                scheduleType: formValues.schedule_type,
                intervalValue: formValues.schedule_type === "interval" ? Number(formValues.interval_value) : null,
                intervalUnit: formValues.schedule_type === "interval" ? formValues.interval_unit : null,
                startTime: formValues.schedule_type === "interval" && needsStartTime ? formValues.start_time : null,
                weekDays: formValues.schedule_type === "interval" && formValues.interval_unit === "week" ? formValues.week_days : null,
                monthDays: formValues.schedule_type === "interval" && formValues.interval_unit === "month" ? formValues.month_days : null,
                runAt: formValues.schedule_type === "once" ? toServerDatetime(onceRunAtRaw) : null,
                assistantId: Number(formValues.assistant_id),
                taskPrompt: formValues.task_prompt.trim(),
                notifyPrompt: formValues.notify_prompt.trim(),
            };
            if (!payload.name) {
                throw new Error("请输入任务名称");
            }
            if (!payload.assistantId) {
                throw new Error("请选择助手");
            }
            if (!payload.taskPrompt) {
                throw new Error("请输入任务指令");
            }
            if (payload.scheduleType === "once" && !onceRunAtRaw) {
                throw new Error("请先选择执行日期和时间");
            }
            if (payload.scheduleType === "once" && !payload.runAt) {
                console.error("[ScheduleWindow] Invalid once runAt when saving task", {
                    rawRunAt: onceRunAtRaw,
                    stateRunAt: formValues.run_at,
                    runAtParts: parseRunAtParts(formValues.run_at),
                    parsedRunAt: payload.runAt,
                });
                throw new Error(`执行时间格式无效：${onceRunAtRaw || "空值"}，请重新选择时间`);
            }
            if (payload.scheduleType === "interval" && (!payload.intervalValue || payload.intervalValue <= 0)) {
                throw new Error("请设置有效的执行周期");
            }
            if (payload.intervalUnit === "week" && (!payload.weekDays || payload.weekDays.length === 0)) {
                throw new Error("请至少选择一个星期几");
            }
            if (payload.intervalUnit === "month" && (!payload.monthDays || payload.monthDays.length === 0)) {
                throw new Error("请至少选择一天");
            }

            if (editingTaskId !== null) {
                const updated = await invoke<ScheduledTask>("update_scheduled_task", {
                    request: { id: editingTaskId, ...payload },
                });
                setActiveTaskId(updated.id);
            } else {
                const created = await invoke<ScheduledTask>("create_scheduled_task", { request: payload });
                setActiveTaskId(created.id);
            }

            toast({
                title: "保存成功",
                description: "定时任务已更新",
            });
            setEditingTaskId(null);
            setIsDialogOpen(false);
            loadTasks();
        } catch (error) {
            const rawMessage = getErrorMessage(error);
            const userMessage = humanizeSaveError(rawMessage);
            console.error("[ScheduleWindow] Failed to save scheduled task", {
                error,
                rawMessage,
                userMessage,
                editingTaskId,
                formValues,
                payload,
            });
            toast({
                title: "保存失败",
                description: userMessage,
                variant: "destructive",
            });
        } finally {
            setIsSaving(false);
        }
    }, [editingTaskId, formValues, loadTasks, toast]);

    const handleToggleEnabled = useCallback(
        async (task: ScheduledTask, value: boolean) => {
            try {
                const updated = await invoke<ScheduledTask>("update_scheduled_task", {
                    request: {
                        id: task.id,
                        name: task.name,
                        isEnabled: value,
                        scheduleType: task.scheduleType,
                        intervalValue: task.intervalValue ?? null,
                        intervalUnit: task.intervalUnit ?? null,
                        startTime: task.startTime ?? null,
                        weekDays: task.weekDays ?? null,
                        monthDays: task.monthDays ?? null,
                        runAt: task.runAt ? toServerDatetime(toLocalDatetimeInput(task.runAt)) : null,
                        assistantId: task.assistantId,
                        taskPrompt: task.taskPrompt,
                        notifyPrompt: task.notifyPrompt,
                    },
                });
                setTasks((prev) =>
                    prev.map((item) => (item.id === task.id ? { ...item, ...updated, isEnabled: value } : item))
                );
            } catch (error) {
                toast({
                    title: "更新失败",
                    description: error as string,
                    variant: "destructive",
                });
            }
        },
        [toast]
    );

    const handleRunNow = useCallback(
        async (task: ScheduledTask) => {
            setIsRunning(true);
            try {
                void loadRuns(task.id, { silent: true });
                const result = await invoke<{ success: boolean; notify: boolean; summary?: string; error?: string }>(
                    "run_scheduled_task_now",
                    { taskId: task.id }
                );
                if (!result.success) {
                    throw new Error(result.error || "执行失败");
                }
                toast({
                    title: result.notify ? "任务执行完成" : "任务执行完成（未通知）",
                    description: result.summary ?? "已完成执行",
                });
                await loadTasks();
                await loadRuns(task.id, { silent: true });
            } catch (error) {
                toast({
                    title: "执行失败",
                    description: error as string,
                    variant: "destructive",
                });
            } finally {
                setIsRunning(false);
            }
        },
        [loadRuns, loadTasks, toast]
    );

    const runningRun = useMemo(
        () => runEntries.find((run) => run.status === "running") ?? null,
        [runEntries]
    );

    const handleStopRun = useCallback(async () => {
        if (!selectedTask || !runningRun) {
            return;
        }
        setIsStopping(true);
        try {
            await invoke<boolean>("stop_scheduled_task_run", {
                taskId: selectedTask.id,
                runId: runningRun.runId,
            });
            toast({
                title: "已发送停止请求",
                description: `正在停止运行 ${runningRun.runId.slice(0, 8)}...`,
            });
            await loadRuns(selectedTask.id, { silent: true });
            await loadLogs(selectedTask.id, runningRun.runId, { silent: true });
        } catch (error) {
            toast({
                title: "停止失败",
                description: getErrorMessage(error),
                variant: "destructive",
            });
        } finally {
            setIsStopping(false);
        }
    }, [loadLogs, loadRuns, runningRun, selectedTask, toast]);

    const parseToolLogPayload = useCallback((log: ScheduledTaskLog): ScheduledTaskToolLogPayload | null => {
        if (log.messageType !== "tool_call" && log.messageType !== "tool_result") {
            return null;
        }
        try {
            const parsed = JSON.parse(log.content) as ScheduledTaskToolLogPayload;
            if (!parsed || typeof parsed !== "object") {
                return null;
            }
            return parsed;
        } catch {
            return null;
        }
    }, []);

    const selectedToolPayload = useMemo(() => {
        if (!selectedToolLog) {
            return null;
        }
        return parseToolLogPayload(selectedToolLog);
    }, [parseToolLogPayload, selectedToolLog]);

    const scheduleDescription = useMemo(() => {
        if (!selectedTask) return "";
        if (selectedTask.scheduleType === "once") {
            return selectedTask.runAt ? `执行时间: ${new Date(selectedTask.runAt).toLocaleString()}` : "未设置时间";
        }
        const value = selectedTask.intervalValue ?? 1;
        const unit = intervalUnitLabels[selectedTask.intervalUnit ?? "hour"] ?? selectedTask.intervalUnit ?? "";
        let desc = `每 ${value} ${unit}`;

        if (selectedTask.intervalUnit === "week" && selectedTask.weekDays?.length) {
            const days = selectedTask.weekDays.map(d => weekDayLabels[d] ?? d).join("、");
            desc += ` (${days})`;
        } else if (selectedTask.intervalUnit === "month" && selectedTask.monthDays?.length) {
            const days = selectedTask.monthDays.join("、");
            desc += ` (${days}日)`;
        }

        if (["day", "week", "month"].includes(selectedTask.intervalUnit ?? "") && selectedTask.startTime) {
            desc += ` ${selectedTask.startTime}`;
        }

        return desc + " 执行";
    }, [selectedTask]);

    const hasDetailPrompts = useMemo(() => {
        return Boolean(selectedTask?.taskPrompt || selectedTask?.notifyPrompt);
    }, [selectedTask]);

    const assistantName = useMemo(() => {
        if (!selectedTask) return "";
        return assistants.find((assistant) => assistant.id === selectedTask.assistantId)?.name ?? "";
    }, [assistants, selectedTask]);

    const selectOptions = useMemo(
        () =>
            tasks.map((task) => ({
                id: task.id.toString(),
                label: task.name,
                icon: <Clock className="h-4 w-4" />,
            })),
        [tasks]
    );

    const handleSelectFromDropdown = useCallback(
        (taskId: string) => {
            const task = tasks.find((item) => item.id.toString() === taskId);
            if (task) {
                setActiveTaskId(task.id);
            }
        },
        [tasks]
    );

    const addButton = useMemo(
        () => (
            <div className="flex items-center gap-2">
                <Button variant="ghost" size="icon" onClick={() => setIsHelpDialogOpen(true)} title="使用说明">
                    <HelpCircle className="h-4 w-4" />
                </Button>
                <Button variant="outline" size="icon" onClick={handleRefresh} disabled={isRefreshing} title="刷新">
                    <RefreshCw className={isRefreshing ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
                </Button>
                <Button size="icon" onClick={openCreateDialog} title="新建任务">
                    <Plus className="h-4 w-4" />
                </Button>
            </div>
        ),
        [handleRefresh, isRefreshing, openCreateDialog]
    );

    const sidebar = useMemo(
        () => (
            <SidebarList
                title="定时任务"
                description="管理自动执行的任务调度"
                icon={<Clock className="h-5 w-5" />}
                addButton={addButton}
            >
                <div className="flex items-center justify-between text-xs text-muted-foreground">
                    <span>共 {tasks.length} 个</span>
                    {isLoading && (
                        <span className="flex items-center gap-1">
                            <RefreshCw className="h-3 w-3 animate-spin" />
                            加载中
                        </span>
                    )}
                </div>
                {isLoading ? (
                    <div className="flex items-center justify-center py-6 text-muted-foreground text-sm">
                        <RefreshCw className="h-3.5 w-3.5 animate-spin mr-2" />
                        加载中...
                    </div>
                ) : tasks.length === 0 ? (
                    <div className="text-sm text-muted-foreground py-6 text-center">暂无定时任务</div>
                ) : (
                    <div className="space-y-2">
                        {tasks.map((task) => {
                            const isSelected = activeTaskId === task.id;
                            const subTextClass = isSelected ? "text-primary-foreground/80" : "text-muted-foreground";
                            return (
                                <ListItemButton
                                    key={task.id}
                                    isSelected={isSelected}
                                    onClick={() => setActiveTaskId(task.id)}
                                    className="h-auto items-start py-2"
                                >
                                    <div className="flex items-start w-full gap-2">
                                        <div className="min-w-0 flex-1">
                                            <div className={`text-sm font-medium truncate ${isSelected ? "text-primary-foreground" : "text-foreground"}`}>
                                                {task.name}
                                            </div>
                                            <div className={`text-xs mt-0.5 ${subTextClass}`}>
                                                {task.scheduleType === "once" ? "单次" : "周期"} · {task.isEnabled ? "已启用" : "已停用"}
                                            </div>
                                            {task.nextRunAt && (
                                                <div className={`text-xs mt-1 flex items-center gap-1 ${subTextClass}`}>
                                                    <Clock className="h-3 w-3" />
                                                    下次: {new Date(task.nextRunAt).toLocaleString()}
                                                </div>
                                            )}
                                        </div>
                                    </div>
                                </ListItemButton>
                            );
                        })}
                    </div>
                )}
            </SidebarList>
        ),
        [addButton, tasks, isLoading, activeTaskId]
    );

    const renderLogTag = useCallback((messageType: string) => {
        const label = logTypeLabels[messageType] ?? messageType;
        const isError = messageType === "error";
        return (
            <span
                className={`text-xs px-2 py-0.5 rounded-full border ${isError
                    ? "border-destructive/40 text-destructive"
                    : "border-muted-foreground/30 text-muted-foreground"
                    }`}
            >
                {label}
            </span>
        );
    }, []);

    const renderRunStatus = useCallback((status: ScheduledTaskRun["status"]) => {
        if (status === "failed") {
            return "text-destructive border-destructive/40";
        }
        if (status === "running") {
            return "text-amber-500 border-amber-300/60";
        }
        return "text-emerald-600 border-emerald-300/60";
    }, []);

    const emptyState = useMemo(
        () => (
            <EmptyState
                icon={<Clock className="h-8 w-8 text-muted-foreground" />}
                title="还没有定时任务"
                description="创建你的第一个定时任务，自动执行助手对话"
                action={
                    <Button size="icon" onClick={openCreateDialog} title="新建任务">
                        <Plus className="h-4 w-4" />
                    </Button>
                }
            />
        ),
        [openCreateDialog]
    );

    const content = useMemo(
        () =>
            selectedTask ? (
                <Card className="shadow-none">
                    <CardHeader className="pb-3">
                        <div className="flex items-start justify-between gap-4">
                            <div>
                                <CardTitle className="text-base">{selectedTask.name}</CardTitle>
                                <p className="text-xs text-muted-foreground mt-1">{scheduleDescription}</p>
                            </div>
                            <div className="flex items-center gap-2">
                                <div className="flex items-center gap-2 text-xs text-muted-foreground">
                                    <span>启用</span>
                                    <Switch
                                        checked={selectedTask.isEnabled}
                                        onCheckedChange={(value) => handleToggleEnabled(selectedTask, value)}
                                    />
                                </div>
                                <Button variant="outline" size="icon" onClick={() => openEditDialog(selectedTask)} title="编辑">
                                    <Pencil className="h-4 w-4" />
                                </Button>
                                <Button
                                    variant="outline"
                                    size="icon"
                                    onClick={() => handleRunNow(selectedTask)}
                                    disabled={isRunning}
                                    title="执行"
                                >
                                    <Play className="h-4 w-4" />
                                </Button>
                                {runningRun && (
                                    <Button
                                        variant="outline"
                                        size="icon"
                                        onClick={handleStopRun}
                                        disabled={isStopping}
                                        title="停止当前执行"
                                    >
                                        <Square className="h-4 w-4" />
                                    </Button>
                                )}
                                <Button
                                    variant="destructive"
                                    size="icon"
                                    onClick={() => setDeleteTarget(selectedTask)}
                                    title="删除"
                                >
                                    <Trash2 className="h-4 w-4" />
                                </Button>
                            </div>
                        </div>
                    </CardHeader>
                    <CardContent className="space-y-3 pt-0">
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-3 text-xs">
                            <div className="flex items-center gap-2">
                                <Calendar className="h-3.5 w-3.5 text-muted-foreground" />
                                <span className="text-muted-foreground">下次执行:</span>
                                <span>
                                    {selectedTask.nextRunAt
                                        ? new Date(selectedTask.nextRunAt).toLocaleString()
                                        : "未设置"}
                                </span>
                            </div>
                            <div className="flex items-center gap-2">
                                <Clock className="h-3.5 w-3.5 text-muted-foreground" />
                                <span className="text-muted-foreground">上次执行:</span>
                                <span>
                                    {selectedTask.lastRunAt
                                        ? new Date(selectedTask.lastRunAt).toLocaleString()
                                        : "未执行"}
                                </span>
                            </div>
                            <div className="flex items-center gap-2">
                                <Bell className="h-3.5 w-3.5 text-muted-foreground" />
                                <span className="text-muted-foreground">执行助手:</span>
                                <span>{assistantName || "未选择"}</span>
                            </div>
                        </div>

                        <div className="space-y-1.5">
                            <div className="text-xs font-semibold">任务指令</div>
                            <div className="rounded bg-muted/30 border p-2 text-xs whitespace-pre-wrap max-h-24 overflow-y-auto">
                                {selectedTask.taskPrompt || (hasDetailPrompts ? "未设置" : "暂无数据")}
                            </div>
                        </div>

                        <div className="space-y-1.5">
                            <div className="text-xs font-semibold">通知判定规则</div>
                            <div className="rounded bg-muted/30 border p-2 text-xs whitespace-pre-wrap max-h-20 overflow-y-auto">
                                {selectedTask.notifyPrompt || "默认规则：有重要信息时通知"}
                            </div>
                        </div>

                        <div className="space-y-1.5">
                            <div className="flex items-center justify-between">
                                <div className="text-xs font-semibold">运行记录</div>
                                <Button
                                    variant="outline"
                                    size="icon"
                                    onClick={() => loadRuns(selectedTask.id)}
                                    disabled={isLogLoading}
                                    title="刷新"
                                >
                                    <RefreshCw className={isLogLoading ? "h-3.5 w-3.5 animate-spin" : "h-3.5 w-3.5"} />
                                </Button>
                            </div>
                            <div className="rounded border bg-muted/10 p-2 space-y-2 max-h-[200px] overflow-y-auto">
                                {isLogLoading ? (
                                    <div className="text-xs text-muted-foreground flex items-center">
                                        <RefreshCw className="h-3 w-3 animate-spin mr-2" />
                                        加载记录...
                                    </div>
                                ) : runEntries.length === 0 ? (
                                    <div className="text-xs text-muted-foreground">暂无运行记录</div>
                                ) : (
                                    runEntries.map((run) => (
                                        <Button
                                            key={run.id}
                                            type="button"
                                            variant="outline"
                                            onClick={() => setSelectedRunId(run.runId)}
                                            className={`w-full text-left justify-start h-auto p-2 space-y-1 transition-colors ${selectedRunId === run.runId
                                                ? "border-primary bg-muted/40"
                                                : "border-muted-foreground/20 hover:bg-muted/30"
                                                }`}
                                        >
                                            <div className="flex items-center justify-between gap-2 w-full">
                                                <div className="flex items-center gap-2">
                                                    <span
                                                        className={`text-[11px] px-2 py-0.5 rounded-full border ${renderRunStatus(
                                                            run.status
                                                        )}`}
                                                    >
                                                        {run.status === "running"
                                                            ? "运行中"
                                                            : run.status === "failed"
                                                                ? "失败"
                                                                : "完成"}
                                                    </span>
                                                    <span className="text-xs text-muted-foreground">
                                                        {new Date(run.startedTime).toLocaleString()}
                                                    </span>
                                                    {run.notify && (
                                                        <span className="text-[11px] px-2 py-0.5 rounded-full border border-emerald-300/60 text-emerald-600">
                                                            已通知
                                                        </span>
                                                    )}
                                                </div>
                                                <span className="text-[10px] text-muted-foreground font-mono truncate">
                                                    {run.runId.slice(0, 8)}
                                                </span>
                                            </div>
                                            <div className="text-[11px] whitespace-pre-wrap break-words text-muted-foreground line-clamp-2">
                                                {run.errorMessage || run.summary || "无摘要"}
                                            </div>
                                        </Button>
                                    ))
                                )}
                            </div>
                        </div>

                        <div className="space-y-1.5">
                            <div className="text-xs font-semibold">运行日志详情</div>
                            <div className="rounded border bg-muted/10 p-2 space-y-2 max-h-[240px] overflow-y-auto">
                                {selectedRunId ? (
                                    isLogLoading ? (
                                        <div className="text-xs text-muted-foreground flex items-center">
                                            <RefreshCw className="h-3 w-3 animate-spin mr-2" />
                                            加载日志...
                                        </div>
                                    ) : logEntries.length === 0 ? (
                                        <div className="text-xs text-muted-foreground">暂无日志详情</div>
                                    ) : (
                                        logEntries.map((log) => (
                                            <div key={log.id} className="border border-muted-foreground/20 rounded-md p-1.5 space-y-0.5">
                                                <div className="flex items-center justify-between gap-2">
                                                    <div className="flex items-center gap-1.5">
                                                        {renderLogTag(log.messageType)}
                                                        <span className="text-[10px] text-muted-foreground">
                                                            {new Date(log.createdTime).toLocaleString()}
                                                        </span>
                                                    </div>
                                                    <span className="text-[10px] text-muted-foreground font-mono truncate">
                                                        {log.runId.slice(0, 8)}
                                                    </span>
                                                </div>
                                                {(() => {
                                                    const toolPayload = parseToolLogPayload(log);
                                                    if (!toolPayload) {
                                                        return (
                                                            <div className="text-[11px] whitespace-pre-wrap break-words">
                                                                {log.content}
                                                            </div>
                                                        );
                                                    }
                                                    const toolText = `${toolPayload.serverName ?? "-"} / ${toolPayload.toolName ?? "-"}`;
                                                    const statusText =
                                                        log.messageType === "tool_result"
                                                            ? (toolPayload.success ? "执行成功" : "执行失败")
                                                            : "准备执行";
                                                    return (
                                                        <div className="space-y-1">
                                                            <div className="text-[11px] whitespace-pre-wrap break-words">
                                                                {statusText}：{toolText}
                                                            </div>
                                                            <Button
                                                                type="button"
                                                                variant="link"
                                                                className="h-auto p-0 text-[11px]"
                                                                onClick={() => setSelectedToolLog(log)}
                                                            >
                                                                查看工具详情（参数/返回）
                                                            </Button>
                                                        </div>
                                                    );
                                                })()}
                                            </div>
                                        ))
                                    )
                                ) : (
                                    <div className="text-xs text-muted-foreground">请选择一条运行记录查看详情</div>
                                )}
                            </div>
                        </div>
                    </CardContent>
                </Card>
            ) : (
                <EmptyState
                    icon={<Calendar className="h-8 w-8 text-muted-foreground" />}
                    title="选择一个任务"
                    description="从左侧列表中选择一个定时任务查看详情"
                    action={
                        <Button size="icon" onClick={openCreateDialog} title="新建任务">
                            <Plus className="h-4 w-4" />
                        </Button>
                    }
                />
            ),
        [
            assistantName,
            handleRunNow,
            handleStopRun,
            hasDetailPrompts,
            isLogLoading,
            isRunning,
            isStopping,
            loadRuns,
            logEntries,
            openCreateDialog,
            openEditDialog,
            parseToolLogPayload,
            renderLogTag,
            renderRunStatus,
            runningRun,
            runEntries,
            scheduleDescription,
            selectedRunId,
            selectedTask,
        ]
    );

    return (
        <div className="flex justify-center items-center h-screen bg-background">
            <div className="bg-card shadow-none w-full h-screen overflow-y-auto">
                <ConfigPageLayout
                    sidebar={sidebar}
                    content={content}
                    selectOptions={selectOptions}
                    selectedOptionId={activeTaskId ? activeTaskId.toString() : undefined}
                    onSelectOption={handleSelectFromDropdown}
                    selectPlaceholder="选择任务"
                    addButton={addButton}
                    emptyState={emptyState}
                    showEmptyState={!isLoading && tasks.length === 0}
                />
            </div>

            <Dialog
                open={isDialogOpen}
                onOpenChange={(open) => {
                    setIsDialogOpen(open);
                    if (!open) {
                        setEditingTaskId(null);
                    }
                }}
            >
                <DialogContent className="sm:max-w-[640px] max-h-[85vh] overflow-y-auto">
                    <DialogHeader>
                        <DialogTitle className="text-base">{editingTaskId !== null ? "编辑任务" : "新建任务"}</DialogTitle>
                    </DialogHeader>
                    <div className="space-y-4 py-2">
                        <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
                            <div className="space-y-1.5">
                                <Label className="text-xs">任务名称</Label>
                                <Input
                                    value={formValues.name}
                                    onChange={(e) => setFormValues((prev) => ({ ...prev, name: e.target.value }))}
                                    placeholder="例如：每日汇总报告"
                                    className="h-8 text-sm"
                                />
                            </div>
                            <div className="space-y-1.5">
                                <Label className="text-xs">选择助手</Label>
                                <Select
                                    value={formValues.assistant_id}
                                    onValueChange={(value) => setFormValues((prev) => ({ ...prev, assistant_id: value }))}
                                >
                                    <SelectTrigger className="h-8 text-sm">
                                        <SelectValue placeholder="选择普通助手" />
                                    </SelectTrigger>
                                    <SelectContent>
                                        {assistantOptions.map((assistant) => (
                                            <SelectItem key={assistant.id} value={assistant.id.toString()}>
                                                {assistant.name}
                                            </SelectItem>
                                        ))}
                                    </SelectContent>
                                </Select>
                            </div>
                        </div>

                        <div className="flex items-center gap-2">
                            <Switch
                                checked={formValues.is_enabled}
                                onCheckedChange={(value) => setFormValues((prev) => ({ ...prev, is_enabled: value }))}
                            />
                            <Label className="text-xs">启用任务</Label>
                        </div>

                        <div className="space-y-2">
                            <Label className="text-xs">执行时间</Label>
                            <RadioGroup
                                value={formValues.schedule_type}
                                onValueChange={(value) =>
                                    setFormValues((prev) => ({ ...prev, schedule_type: value as "once" | "interval" }))
                                }
                                className="flex flex-col gap-3"
                            >
                                <div className="flex items-start gap-2">
                                    <RadioGroupItem value="once" id="schedule-once" className="mt-0.5" />
                                    <div className="flex-1 space-y-1.5">
                                        <Label htmlFor="schedule-once" className="text-xs">指定时间执行一次</Label>
                                        <div className="flex gap-2">
                                            <Popover>
                                                <PopoverTrigger asChild>
                                                    <Button
                                                        type="button"
                                                        variant="outline"
                                                        disabled={formValues.schedule_type !== "once"}
                                                        className="h-8 text-sm justify-start min-w-[180px]"
                                                    >
                                                        {onceSelectedDate ? format(onceSelectedDate, "yyyy/MM/dd") : "选择日期"}
                                                    </Button>
                                                </PopoverTrigger>
                                                <PopoverContent className="w-auto p-0" align="start">
                                                    <DateCalendar
                                                        mode="single"
                                                        selected={onceSelectedDate}
                                                        onSelect={handleOnceDateChange}
                                                    />
                                                </PopoverContent>
                                            </Popover>
                                            <Input
                                                type="time"
                                                step={60}
                                                value={onceTimePart}
                                                onChange={(e) => handleOnceTimeChange(e.target.value)}
                                                disabled={formValues.schedule_type !== "once" || !onceDatePart}
                                                className="h-8 text-sm w-28"
                                            />
                                        </div>
                                        <div className="text-[11px] text-muted-foreground">
                                            {onceRunAtParts ? `已选择: ${onceRunAtParts.date} ${onceRunAtParts.time}` : "请先选择日期和时间"}
                                        </div>
                                    </div>
                                </div>
                                <div className="flex items-start gap-2">
                                    <RadioGroupItem value="interval" id="schedule-interval" className="mt-0.5" />
                                    <div className="flex-1 space-y-2">
                                        <Label htmlFor="schedule-interval" className="text-xs">按周期重复执行</Label>
                                        <div className="flex gap-2">
                                            <Input
                                                type="number"
                                                min={1}
                                                value={formValues.interval_value}
                                                onChange={(e) =>
                                                    setFormValues((prev) => ({ ...prev, interval_value: e.target.value }))
                                                }
                                                disabled={formValues.schedule_type !== "interval"}
                                                className="h-8 text-sm w-20"
                                            />
                                            <Select
                                                value={formValues.interval_unit}
                                                onValueChange={(value) =>
                                                    setFormValues((prev) => ({ ...prev, interval_unit: value }))
                                                }
                                                disabled={formValues.schedule_type !== "interval"}
                                            >
                                                <SelectTrigger className="w-24 h-8 text-sm">
                                                    <SelectValue />
                                                </SelectTrigger>
                                                <SelectContent>
                                                    {Object.entries(intervalUnitLabels).map(([value, label]) => (
                                                        <SelectItem key={value} value={value}>
                                                            {label}
                                                        </SelectItem>
                                                    ))}
                                                </SelectContent>
                                            </Select>
                                        </div>

                                        {/* Day: start time */}
                                        {formValues.schedule_type === "interval" && formValues.interval_unit === "day" && (
                                            <div className="flex items-center gap-2 mt-2">
                                                <Label className="text-xs text-muted-foreground">开始时间:</Label>
                                                <Input
                                                    type="time"
                                                    value={formValues.start_time}
                                                    onChange={(e) => setFormValues((prev) => ({ ...prev, start_time: e.target.value }))}
                                                    className="h-8 text-sm w-28"
                                                />
                                            </div>
                                        )}

                                        {/* Week: weekday checkboxes + start time */}
                                        {formValues.schedule_type === "interval" && formValues.interval_unit === "week" && (
                                            <div className="space-y-2 mt-2">
                                                <Label className="text-xs text-muted-foreground">选择星期几:</Label>
                                                <div className="flex flex-wrap gap-3">
                                                    {Object.entries(weekDayLabels).map(([value, label]) => {
                                                        const dayNum = Number(value);
                                                        const isChecked = formValues.week_days.includes(dayNum);
                                                        return (
                                                            <label key={value} className="flex items-center gap-1.5 cursor-pointer">
                                                                <Checkbox
                                                                    checked={isChecked}
                                                                    onCheckedChange={(checked) => {
                                                                        setFormValues((prev) => ({
                                                                            ...prev,
                                                                            week_days: checked
                                                                                ? [...prev.week_days, dayNum].sort()
                                                                                : prev.week_days.filter((d) => d !== dayNum),
                                                                        }));
                                                                    }}
                                                                />
                                                                <span className="text-xs">{label}</span>
                                                            </label>
                                                        );
                                                    })}
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <Label className="text-xs text-muted-foreground">开始时间:</Label>
                                                    <Input
                                                        type="time"
                                                        value={formValues.start_time}
                                                        onChange={(e) => setFormValues((prev) => ({ ...prev, start_time: e.target.value }))}
                                                        className="h-8 text-sm w-28"
                                                    />
                                                </div>
                                            </div>
                                        )}

                                        {/* Month: day checkboxes + start time */}
                                        {formValues.schedule_type === "interval" && formValues.interval_unit === "month" && (
                                            <div className="space-y-2 mt-2">
                                                <Label className="text-xs text-muted-foreground">选择日期 (1-31):</Label>
                                                <div className="flex flex-wrap gap-1.5">
                                                    {Array.from({ length: 31 }, (_, i) => i + 1).map((day) => {
                                                        const isChecked = formValues.month_days.includes(day);
                                                        return (
                                                            <Button
                                                                key={day}
                                                                type="button"
                                                                variant="outline"
                                                                size="sm"
                                                                onClick={() => {
                                                                    setFormValues((prev) => ({
                                                                        ...prev,
                                                                        month_days: isChecked
                                                                            ? prev.month_days.filter((d) => d !== day)
                                                                            : [...prev.month_days, day].sort((a, b) => a - b),
                                                                    }));
                                                                }}
                                                                className={`w-7 h-7 p-0 text-xs transition-colors ${isChecked
                                                                    ? "bg-primary text-primary-foreground border-primary hover:bg-primary/90"
                                                                    : "border-input hover:bg-muted"
                                                                    }`}
                                                            >
                                                                {day}
                                                            </Button>
                                                        );
                                                    })}
                                                </div>
                                                <div className="flex items-center gap-2">
                                                    <Label className="text-xs text-muted-foreground">开始时间:</Label>
                                                    <Input
                                                        type="time"
                                                        value={formValues.start_time}
                                                        onChange={(e) => setFormValues((prev) => ({ ...prev, start_time: e.target.value }))}
                                                        className="h-8 text-sm w-28"
                                                    />
                                                </div>
                                            </div>
                                        )}
                                    </div>
                                </div>
                            </RadioGroup>
                        </div>

                        <div className="space-y-1.5">
                            <Label className="text-xs">任务指令</Label>
                            <Textarea
                                rows={3}
                                value={formValues.task_prompt}
                                onChange={(e) => setFormValues((prev) => ({ ...prev, task_prompt: e.target.value }))}
                                placeholder="描述要执行的任务和结果提取要求"
                                className="text-sm"
                            />
                        </div>
                        <div className="space-y-1.5">
                            <Label className="text-xs">通知判定规则 <span className="text-muted-foreground">(留空使用默认规则)</span></Label>
                            <Textarea
                                rows={2}
                                value={formValues.notify_prompt}
                                onChange={(e) => setFormValues((prev) => ({ ...prev, notify_prompt: e.target.value }))}
                                placeholder="例如：如果结果包含错误或异常则通知"
                                className="text-sm"
                            />
                        </div>
                    </div>
                    <DialogFooter className="gap-2">
                        <Button
                            variant="outline"
                            size="sm"
                            onClick={() => {
                                setEditingTaskId(null);
                                setIsDialogOpen(false);
                            }}
                        >
                            取消
                        </Button>
                        <Button size="sm" onClick={handleSave} disabled={isSaving}>
                            {isSaving ? "保存中..." : "保存任务"}
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>

            <Dialog open={!!selectedToolLog} onOpenChange={(open) => !open && setSelectedToolLog(null)}>
                <DialogContent className="sm:max-w-[680px] max-h-[80vh] overflow-y-auto">
                    <DialogHeader>
                        <DialogTitle className="text-base">工具调用详情</DialogTitle>
                    </DialogHeader>
                    {selectedToolPayload ? (
                        <div className="space-y-3 text-xs">
                            <div className="rounded border bg-muted/20 p-2">
                                <div><span className="text-muted-foreground">工具：</span>{selectedToolPayload.serverName ?? "-"} / {selectedToolPayload.toolName ?? "-"}</div>
                                <div><span className="text-muted-foreground">Call ID：</span>{selectedToolPayload.callId ?? "-"}</div>
                                {"success" in selectedToolPayload && (
                                    <div>
                                        <span className="text-muted-foreground">状态：</span>
                                        {selectedToolPayload.success ? "成功" : "失败"}
                                    </div>
                                )}
                            </div>
                            <div className="space-y-1">
                                <div className="font-medium">参数</div>
                                <pre className="rounded border bg-muted/20 p-2 whitespace-pre-wrap break-all max-h-44 overflow-auto">
                                    {formatToolLogValue(selectedToolPayload.parameters)}
                                </pre>
                            </div>
                            {"result" in selectedToolPayload && (
                                <div className="space-y-1">
                                    <div className="font-medium">返回结果</div>
                                    <pre className="rounded border bg-muted/20 p-2 whitespace-pre-wrap break-all max-h-56 overflow-auto">
                                        {formatToolLogValue(selectedToolPayload.result)}
                                    </pre>
                                </div>
                            )}
                        </div>
                    ) : (
                        <pre className="rounded border bg-muted/20 p-2 text-xs whitespace-pre-wrap break-all max-h-[60vh] overflow-auto">
                            {selectedToolLog?.content ?? ""}
                        </pre>
                    )}
                </DialogContent>
            </Dialog>

            {/* Help Dialog */}
            <Dialog open={isHelpDialogOpen} onOpenChange={setIsHelpDialogOpen}>
                <DialogContent className="sm:max-w-[500px]">
                    <DialogHeader>
                        <DialogTitle className="text-base">定时任务使用说明</DialogTitle>
                    </DialogHeader>
                    <div className="space-y-4 text-sm">
                        <div>
                            <h4 className="font-medium mb-1">基本说明</h4>
                            <p className="text-muted-foreground text-xs leading-relaxed">
                                定时任务会在指定时间自动执行 AI 助手对话。任务仅在程序运行时执行，错过的任务会在下次启动时立即执行一次。因为执行任务的是AI，所以<strong>配置更好的助手Prompt、工具、模型等就能够大大提高任务的准确性和效率。</strong>
                            </p>
                        </div>
                        <div>
                            <h4 className="font-medium mb-1">执行周期</h4>
                            <ul className="text-xs text-muted-foreground space-y-1 list-disc list-inside">
                                <li><strong>分钟/小时</strong>：按固定间隔重复执行</li>
                                <li><strong>天</strong>：每 N 天在指定时间执行</li>
                                <li><strong>周</strong>：选择星期几，在指定时间执行</li>
                                <li><strong>月</strong>：选择每月几号，在指定时间执行</li>
                            </ul>
                        </div>
                        <div>
                            <h4 className="font-medium mb-1">通知判定</h4>
                            <p className="text-muted-foreground text-xs leading-relaxed">
                                任务执行完成后，系统会根据通知判定规则决定是否弹出通知。留空则使用默认规则（有重要信息时通知）。
                                你只需要描述判定逻辑，例如："如果结果能够确定是xxx则进行通知"。
                            </p>
                        </div>
                        <div>
                            <h4 className="font-medium mb-1">使用示例</h4>
                            <ul className="text-xs text-muted-foreground space-y-1 list-disc list-inside">
                                <li>每天 9:00 执行"获取工作任务"</li>
                                <li>每周五 17:00 执行"读取git仓库提交情况生成周报"</li>
                                <li>每月 1 号和 15 号执行"生成半月报告"</li>
                            </ul>
                        </div>
                    </div>
                    <DialogFooter>
                        <Button size="sm" onClick={() => setIsHelpDialogOpen(false)}>
                            知道了
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>

            <ConfirmDialog
                isOpen={!!deleteTarget}
                title="删除任务"
                confirmText={`确认删除定时任务 “${deleteTarget?.name ?? ""}” 吗？`}
                onCancel={() => setDeleteTarget(null)}
                onConfirm={async () => {
                    if (!deleteTarget) return;
                    try {
                        await invoke("delete_scheduled_task", { taskId: deleteTarget.id });
                        setDeleteTarget(null);
                        if (activeTaskId === deleteTarget.id) {
                            setActiveTaskId(null);
                        }
                        toast({ title: "删除成功" });
                        loadTasks();
                    } catch (error) {
                        toast({
                            title: "删除失败",
                            description: error as string,
                            variant: "destructive",
                        });
                    }
                }}
            />
        </div>
    );
}
