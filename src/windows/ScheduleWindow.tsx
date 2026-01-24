import { useCallback, useEffect, useMemo, useState } from "react";
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
import ConfirmDialog from "@/components/ConfirmDialog";
import { AssistantListItem } from "@/data/Assistant";
import { useAssistantListListener } from "@/hooks/useAssistantListListener";
import { Calendar, Clock, Plus, RefreshCw, Trash2, Pencil, Play, Bell, HelpCircle } from "lucide-react";

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

const toServerDatetime = (value: string) => {
    if (!value) return "";
    return format(new Date(value), "yyyy-MM-dd HH:mm:ss");
};

export default function ScheduleWindow() {
    useTheme();
    const { toast } = useToast();

    const [tasks, setTasks] = useState<ScheduledTask[]>([]);
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [activeTaskId, setActiveTaskId] = useState<number | null>(null);
    const [isDialogOpen, setIsDialogOpen] = useState(false);
    const [isHelpDialogOpen, setIsHelpDialogOpen] = useState(false);
    const [isSaving, setIsSaving] = useState(false);
    const [isRunning, setIsRunning] = useState(false);
    const [isRefreshing, setIsRefreshing] = useState(false);
    const [runEntries, setRunEntries] = useState<ScheduledTaskRun[]>([]);
    const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
    const [logEntries, setLogEntries] = useState<ScheduledTaskLog[]>([]);
    const [isLogLoading, setIsLogLoading] = useState(false);
    const [deleteTarget, setDeleteTarget] = useState<ScheduledTask | null>(null);
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
        async (taskId?: number | null) => {
            if (!taskId) {
                setRunEntries([]);
                setSelectedRunId(null);
                return;
            }
            setIsLogLoading(true);
            try {
                const result = await invoke<{ runs: ScheduledTaskRun[] }>("list_scheduled_task_runs", {
                    taskId,
                    limit: 50,
                });
                setRunEntries(result.runs);
                setSelectedRunId(result.runs[0]?.runId ?? null);
            } catch (error) {
                console.error("加载定时任务运行记录失败:", error);
                setRunEntries([]);
                setSelectedRunId(null);
            } finally {
                setIsLogLoading(false);
            }
        },
        []
    );

    const loadLogs = useCallback(
        async (taskId?: number | null, runId?: string | null) => {
            if (!taskId || !runId) {
                setLogEntries([]);
                return;
            }
            setIsLogLoading(true);
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
                setIsLogLoading(false);
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
            setActiveTaskId(task.id);
            setIsDialogOpen(true);
        },
        []
    );

    const handleSave = useCallback(async () => {
        setIsSaving(true);
        try {
            const needsStartTime = ["day", "week", "month"].includes(formValues.interval_unit);
            const payload = {
                name: formValues.name.trim(),
                isEnabled: formValues.is_enabled,
                scheduleType: formValues.schedule_type,
                intervalValue: formValues.schedule_type === "interval" ? Number(formValues.interval_value) : null,
                intervalUnit: formValues.schedule_type === "interval" ? formValues.interval_unit : null,
                startTime: formValues.schedule_type === "interval" && needsStartTime ? formValues.start_time : null,
                weekDays: formValues.schedule_type === "interval" && formValues.interval_unit === "week" ? formValues.week_days : null,
                monthDays: formValues.schedule_type === "interval" && formValues.interval_unit === "month" ? formValues.month_days : null,
                runAt: formValues.schedule_type === "once" ? toServerDatetime(formValues.run_at) : null,
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
            if (payload.scheduleType === "once" && !payload.runAt) {
                throw new Error("请设置执行时间");
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

            if (selectedTask && selectedTask.id === activeTaskId) {
                const updated = await invoke<ScheduledTask>("update_scheduled_task", {
                    request: { id: selectedTask.id, ...payload },
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
            setIsDialogOpen(false);
            loadTasks();
        } catch (error) {
            toast({
                title: "保存失败",
                description: error as string,
                variant: "destructive",
            });
        } finally {
            setIsSaving(false);
        }
    }, [activeTaskId, formValues, loadTasks, selectedTask, toast]);

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
                await loadRuns(task.id);
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
        [loadTasks, toast]
    );

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

    const renderLogTag = useCallback((messageType: string) => {
        const label = logTypeLabels[messageType] ?? messageType;
        const isError = messageType === "error";
        return (
            <span
                className={`text-xs px-2 py-0.5 rounded-full border ${
                    isError
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

    return (
        <div className="flex flex-col h-screen bg-background p-4">
            <div className="flex items-center justify-between gap-4 mb-4">
                <div>
                    <h1 className="text-xl font-bold">定时任务</h1>
                    <p className="text-sm text-muted-foreground">管理自动执行的任务调度</p>
                </div>
                <div className="flex items-center gap-2">
                    <Button variant="ghost" size="icon" onClick={() => setIsHelpDialogOpen(true)} title="使用说明">
                        <HelpCircle className="h-4 w-4" />
                    </Button>
                    <Button variant="outline" size="sm" onClick={handleRefresh} disabled={isRefreshing}>
                        <RefreshCw className={isRefreshing ? "mr-1.5 h-3.5 w-3.5 animate-spin" : "mr-1.5 h-3.5 w-3.5"} />
                        刷新
                    </Button>
                    <Button size="sm" onClick={openCreateDialog}>
                        <Plus className="mr-1.5 h-3.5 w-3.5" />
                        新建任务
                    </Button>
                </div>
            </div>

            <div className="grid grid-cols-1 lg:grid-cols-[280px_1fr] gap-4 flex-1 overflow-hidden">
                <div className="border rounded-lg p-3 overflow-y-auto">
                    <div className="flex items-center justify-between mb-3">
                        <h2 className="text-sm font-semibold">任务列表</h2>
                        <span className="text-xs text-muted-foreground">共 {tasks.length} 个</span>
                    </div>
                    {isLoading ? (
                        <div className="flex items-center justify-center py-8 text-muted-foreground text-sm">
                            <RefreshCw className="h-3.5 w-3.5 animate-spin mr-2" />
                            加载中...
                        </div>
                    ) : tasks.length === 0 ? (
                        <div className="text-sm text-muted-foreground py-6 text-center">暂无定时任务</div>
                    ) : (
                        <div className="space-y-1.5">
                            {tasks.map((task) => (
                                <div
                                    key={task.id}
                                    className={`rounded-lg border px-2.5 py-2 cursor-pointer transition-colors ${
                                        activeTaskId === task.id ? "border-primary bg-muted/40" : "hover:bg-muted/30"
                                    }`}
                                    onClick={() => setActiveTaskId(task.id)}
                                >
                                    <div className="flex items-center justify-between gap-2">
                                        <div className="min-w-0">
                                            <div className="text-sm font-medium truncate">{task.name}</div>
                                            <div className="text-xs text-muted-foreground mt-0.5">
                                                {task.scheduleType === "once" ? "单次" : "周期"} ·{" "}
                                                {task.isEnabled ? "已启用" : "已停用"}
                                            </div>
                                        </div>
                                        <Switch
                                            checked={task.isEnabled}
                                            onCheckedChange={(value) => handleToggleEnabled(task, value)}
                                            onClick={(event) => event.stopPropagation()}
                                        />
                                    </div>
                                    {task.nextRunAt && (
                                        <div className="text-xs text-muted-foreground mt-1.5 flex items-center gap-1">
                                            <Clock className="h-3 w-3" />
                                            下次: {new Date(task.nextRunAt).toLocaleString()}
                                        </div>
                                    )}
                                </div>
                            ))}
                        </div>
                    )}
                </div>

                <div className="overflow-y-auto">
                    {selectedTask ? (
                        <Card>
                            <CardHeader className="pb-3">
                                <div className="flex items-start justify-between gap-4">
                                    <div>
                                        <CardTitle className="text-base">{selectedTask.name}</CardTitle>
                                        <p className="text-xs text-muted-foreground mt-1">{scheduleDescription}</p>
                                    </div>
                                    <div className="flex items-center gap-1.5">
                                        <Button variant="outline" size="sm" onClick={() => openEditDialog(selectedTask)}>
                                            <Pencil className="mr-1.5 h-3.5 w-3.5" />
                                            编辑
                                        </Button>
                                        <Button
                                            variant="outline"
                                            size="sm"
                                            onClick={() => handleRunNow(selectedTask)}
                                            disabled={isRunning}
                                        >
                                            <Play className="mr-1.5 h-3.5 w-3.5" />
                                            执行
                                        </Button>
                                        <Button
                                            variant="destructive"
                                            size="sm"
                                            onClick={() => setDeleteTarget(selectedTask)}
                                        >
                                            <Trash2 className="h-3.5 w-3.5" />
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
                                            size="sm"
                                            onClick={() => loadRuns(selectedTask.id)}
                                            disabled={isLogLoading}
                                        >
                                            <RefreshCw className={isLogLoading ? "mr-1.5 h-3 w-3 animate-spin" : "mr-1.5 h-3 w-3"} />
                                            刷新
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
                                                <button
                                                    key={run.id}
                                                    type="button"
                                                    onClick={() => setSelectedRunId(run.runId)}
                                                    className={`w-full text-left border rounded-md p-2 space-y-1 transition-colors ${
                                                        selectedRunId === run.runId
                                                            ? "border-primary bg-muted/40"
                                                            : "border-muted-foreground/20 hover:bg-muted/30"
                                                    }`}
                                                >
                                                    <div className="flex items-center justify-between gap-2">
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
                                                </button>
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
                                                        <div className="text-[11px] whitespace-pre-wrap break-words">
                                                            {log.content}
                                                        </div>
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
                        <div className="h-full flex items-center justify-center text-sm text-muted-foreground">
                            请选择一个任务查看详情
                        </div>
                    )}
                </div>
            </div>

            <Dialog open={isDialogOpen} onOpenChange={setIsDialogOpen}>
                <DialogContent className="sm:max-w-[640px] max-h-[85vh] overflow-y-auto">
                    <DialogHeader>
                        <DialogTitle className="text-base">{selectedTask && selectedTask.id === activeTaskId ? "编辑任务" : "新建任务"}</DialogTitle>
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
                                        <Input
                                            type="datetime-local"
                                            value={formValues.run_at}
                                            onChange={(e) => setFormValues((prev) => ({ ...prev, run_at: e.target.value }))}
                                            disabled={formValues.schedule_type !== "once"}
                                            className="h-8 text-sm"
                                        />
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
                                                            <button
                                                                key={day}
                                                                type="button"
                                                                onClick={() => {
                                                                    setFormValues((prev) => ({
                                                                        ...prev,
                                                                        month_days: isChecked
                                                                            ? prev.month_days.filter((d) => d !== day)
                                                                            : [...prev.month_days, day].sort((a, b) => a - b),
                                                                    }));
                                                                }}
                                                                className={`w-7 h-7 text-xs rounded border transition-colors ${
                                                                    isChecked
                                                                        ? "bg-primary text-primary-foreground border-primary"
                                                                        : "border-input hover:bg-muted"
                                                                }`}
                                                            >
                                                                {day}
                                                            </button>
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
                        <Button variant="outline" size="sm" onClick={() => setIsDialogOpen(false)}>
                            取消
                        </Button>
                        <Button size="sm" onClick={handleSave} disabled={isSaving}>
                            {isSaving ? "保存中..." : "保存任务"}
                        </Button>
                    </DialogFooter>
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
                                定时任务会在指定时间自动执行 AI 助手对话。任务仅在程序运行时执行，错过的任务会在下次启动时立即执行一次。
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
                                你只需要描述判定逻辑，例如："如果结果包含错误则通知"。
                            </p>
                        </div>
                        <div>
                            <h4 className="font-medium mb-1">使用示例</h4>
                            <ul className="text-xs text-muted-foreground space-y-1 list-disc list-inside">
                                <li>每天 9:00 执行"查询今日天气"</li>
                                <li>每周一、三、五 18:00 执行"汇总本周工作"</li>
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
