import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useTheme } from "@/hooks/useTheme";
import { useToast } from "@/hooks/use-toast";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { ConfigPageLayout, SidebarList, ListItemButton, EmptyState } from "@/components/common";
import ConfirmDialog from "@/components/ConfirmDialog";
import { AssistantListItem } from "@/data/Assistant";
import { useAssistantListListener } from "@/hooks/useAssistantListListener";
import { SCHEDULE_FOCUS_TASK_EVENT, consumePendingScheduleFocus, type ScheduleFocusTaskPayload } from "@/lib/scheduleTaskFocus";
import ScheduleTaskDetail, { RunStatusDot } from "@/components/schedule/ScheduleTaskDetail";
import ScheduleTaskEditor from "@/components/schedule/ScheduleTaskEditor";
import { formatShortDateTime, parseToolLogPayload, taskGroup, taskGroupLabels, type TaskGroup } from "@/components/schedule/scheduleFormat";
import type {
    ScheduledRun,
    ScheduledTask,
    ScheduledTaskInput,
    ScheduledTaskLog,
    ScheduledTaskRunEvent,
} from "@/components/schedule/scheduleTypes";
import { Calendar, Clock, HelpCircle, Plus, RefreshCw } from "lucide-react";

const GROUP_ORDER: TaskGroup[] = ["attention", "active", "paused", "finished"];
const LOG_LIMIT = 500;

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

const formatToolLogValue = (value: unknown): string => {
    if (typeof value === "string") return value;
    try {
        return JSON.stringify(value, null, 2);
    } catch {
        return String(value ?? "");
    }
};

function TaskListRow({ task, selected, onSelect }: { task: ScheduledTask; selected: boolean; onSelect: () => void }) {
    const sub = selected ? "text-primary-foreground/80" : "text-muted-foreground";
    const next = task.enabled && task.upcoming[0] ? `下次 ${formatShortDateTime(task.upcoming[0])}` : task.pausedReason;
    return (
        <ListItemButton isSelected={selected} onClick={onSelect} className="h-auto items-start py-2">
            <div className="w-full min-w-0">
                <div className="flex items-center gap-2">
                    {task.lastRun && <RunStatusDot status={task.lastRun.status} />}
                    <span className={`truncate text-sm font-medium ${selected ? "text-primary-foreground" : "text-foreground"}`}>{task.name}</span>
                </div>
                <div className={`mt-0.5 truncate text-xs ${sub}`}>
                    {task.scheduleText} · {task.assistantName}
                </div>
                {next && (
                    <div className={`mt-0.5 flex items-center gap-1 truncate text-xs ${sub}`}>
                        <Clock className="h-3 w-3 shrink-0" />
                        <span className="truncate">{next}</span>
                    </div>
                )}
            </div>
        </ListItemButton>
    );
}

export default function ScheduleWindow() {
    useTheme("schedule");
    const { toast } = useToast();

    const [tasks, setTasks] = useState<ScheduledTask[]>([]);
    const [assistants, setAssistants] = useState<AssistantListItem[]>([]);
    const [isLoading, setIsLoading] = useState(true);
    const [search, setSearch] = useState("");
    const [activeTaskId, setActiveTaskId] = useState<number | null>(null);
    // 其他窗口（如管家窗口）请求定位的任务；任务列表加载到它之后再选中
    const [pendingFocusTaskId, setPendingFocusTaskId] = useState<number | null>(() => consumePendingScheduleFocus());
    const [editor, setEditor] = useState<{ open: boolean; task: ScheduledTask | null }>({ open: false, task: null });
    const [isSaving, setIsSaving] = useState(false);
    const [isStarting, setIsStarting] = useState(false);
    const [isStopping, setIsStopping] = useState(false);
    const [isHelpOpen, setIsHelpOpen] = useState(false);
    const [runs, setRuns] = useState<ScheduledRun[]>([]);
    const [selectedRunId, setSelectedRunId] = useState<string | null>(null);
    const [logs, setLogs] = useState<ScheduledTaskLog[]>([]);
    const [isRunsLoading, setIsRunsLoading] = useState(false);
    const [isLogsLoading, setIsLogsLoading] = useState(false);
    const [selectedToolLog, setSelectedToolLog] = useState<ScheduledTaskLog | null>(null);
    const [deleteTarget, setDeleteTarget] = useState<ScheduledTask | null>(null);
    const activeTaskIdRef = useRef<number | null>(null);
    activeTaskIdRef.current = activeTaskId;
    const selectedRunIdRef = useRef<string | null>(null);
    selectedRunIdRef.current = selectedRunId;

    const selectedTask = useMemo(() => tasks.find((task) => task.id === activeTaskId) ?? null, [tasks, activeTaskId]);
    const assistantOptions = useMemo(
        () => assistants.filter((assistant) => assistant.assistant_type === 0).map((a) => ({ id: a.id, name: a.name })),
        [assistants],
    );

    const loadTasks = useCallback(async () => {
        try {
            // 管家与各助手创建的任务都在这里统一查看与管理
            const result = await invoke<ScheduledTask[]>("list_scheduled_tasks");
            setTasks(result);
            setActiveTaskId((current) => {
                if (current !== null && result.some((task) => task.id === current)) return current;
                const first = GROUP_ORDER.flatMap((group) => result.filter((task) => taskGroup(task) === group))[0];
                return first?.id ?? null;
            });
        } catch (error) {
            toast({ title: "加载定时任务失败", description: errorText(error), variant: "destructive" });
        } finally {
            setIsLoading(false);
        }
    }, [toast]);

    const loadAssistants = useCallback(async () => {
        try {
            setAssistants(await invoke<AssistantListItem[]>("get_assistants"));
        } catch (error) {
            console.error("加载助手失败:", error);
        }
    }, []);

    useAssistantListListener({ onAssistantListChanged: setAssistants });

    useEffect(() => {
        void loadTasks();
        void loadAssistants();
    }, [loadTasks, loadAssistants]);

    useEffect(() => {
        const unlisten = listen<ScheduleFocusTaskPayload>(SCHEDULE_FOCUS_TASK_EVENT, (event) => {
            // 事件已带上任务 id，清掉同一请求留在 localStorage 里的副本
            consumePendingScheduleFocus();
            setPendingFocusTaskId(event.payload.taskId);
            void loadTasks();
        });
        return () => {
            void unlisten.then((fn) => fn());
        };
    }, [loadTasks]);

    useEffect(() => {
        if (pendingFocusTaskId === null || isLoading) return;
        if (tasks.some((task) => task.id === pendingFocusTaskId)) {
            setActiveTaskId(pendingFocusTaskId);
            setPendingFocusTaskId(null);
        }
    }, [pendingFocusTaskId, tasks, isLoading]);

    const loadRuns = useCallback(async (taskId: number | null, silent = false) => {
        if (!taskId) {
            setRuns([]);
            setSelectedRunId(null);
            return;
        }
        if (!silent) setIsRunsLoading(true);
        try {
            const result = await invoke<ScheduledRun[]>("list_scheduled_task_runs", { taskId, limit: 50 });
            if (activeTaskIdRef.current !== taskId) return;
            setRuns(result);
            setSelectedRunId((prev) => (prev && result.some((run) => run.runId === prev) ? prev : result[0]?.runId ?? null));
        } catch (error) {
            console.error("加载运行记录失败:", error);
            setRuns([]);
        } finally {
            if (!silent) setIsRunsLoading(false);
        }
    }, []);

    const loadLogs = useCallback(async (taskId: number | null, runId: string | null) => {
        if (!taskId || !runId) {
            setLogs([]);
            return;
        }
        setIsLogsLoading(true);
        try {
            setLogs(await invoke<ScheduledTaskLog[]>("list_scheduled_task_logs", { taskId, runId, limit: LOG_LIMIT }));
        } catch (error) {
            console.error("加载运行过程失败:", error);
            setLogs([]);
        } finally {
            setIsLogsLoading(false);
        }
    }, []);

    useEffect(() => {
        void loadRuns(selectedTask?.id ?? null);
    }, [loadRuns, selectedTask?.id]);

    useEffect(() => {
        void loadLogs(selectedTask?.id ?? null, selectedRunId);
    }, [loadLogs, selectedTask?.id, selectedRunId]);

    // 任务与运行的变化：列表整体刷新（状态、下次时间、上次结果都可能变），当前任务的运行记录静默刷新
    useEffect(() => {
        const onRunEvent = (event: { payload: ScheduledTaskRunEvent }) => {
            void loadTasks();
            if (event.payload.taskId === activeTaskIdRef.current) void loadRuns(event.payload.taskId, true);
        };
        const subscriptions = [
            listen("scheduled_task_changed", () => void loadTasks()),
            listen<ScheduledTaskRunEvent>("scheduled_task_run_created", (event) => {
                onRunEvent(event);
                if (event.payload.taskId === activeTaskIdRef.current) setSelectedRunId(event.payload.runId);
            }),
            listen<ScheduledTaskRunEvent>("scheduled_task_run_updated", onRunEvent),
            listen<ScheduledTaskLog>("scheduled_task_log_added", (event) => {
                const log = event.payload;
                if (log.taskId !== activeTaskIdRef.current || log.runId !== selectedRunIdRef.current) return;
                setLogs((prev) => (prev.some((item) => item.id === log.id) ? prev : [...prev, log].slice(-LOG_LIMIT)));
            }),
        ];
        return () => {
            subscriptions.forEach((subscription) => void subscription.then((unlisten) => unlisten()));
        };
    }, [loadRuns, loadTasks]);

    const handleSave = useCallback(
        async (input: ScheduledTaskInput) => {
            setIsSaving(true);
            try {
                const saved = editor.task
                    ? await invoke<ScheduledTask>("update_scheduled_task", { taskId: editor.task.id, patch: input })
                    : await invoke<ScheduledTask>("create_scheduled_task", { input });
                setActiveTaskId(saved.id);
                setEditor({ open: false, task: null });
                toast({ title: editor.task ? "已保存" : "已创建", description: `${saved.scheduleText}；${saved.upcomingText[0] ? `下次 ${saved.upcomingText[0]}` : "未安排运行"}` });
                await loadTasks();
            } catch (error) {
                toast({ title: "保存失败", description: errorText(error), variant: "destructive" });
            } finally {
                setIsSaving(false);
            }
        },
        [editor.task, loadTasks, toast],
    );

    const handleToggleEnabled = useCallback(
        async (task: ScheduledTask, enabled: boolean) => {
            try {
                const updated = await invoke<ScheduledTask>("set_scheduled_task_enabled", { taskId: task.id, enabled });
                setTasks((prev) => prev.map((item) => (item.id === task.id ? updated : item)));
            } catch (error) {
                toast({ title: enabled ? "启用失败" : "暂停失败", description: errorText(error), variant: "destructive" });
            }
        },
        [toast],
    );

    const handleRunNow = useCallback(
        async (task: ScheduledTask) => {
            setIsStarting(true);
            try {
                const runId = await invoke<string>("run_scheduled_task_now", { taskId: task.id });
                setSelectedRunId(runId);
                toast({ title: "已开始运行", description: `结果会按设置发到${task.deliverTargetText}` });
                await loadRuns(task.id, true);
            } catch (error) {
                toast({ title: "无法运行", description: errorText(error), variant: "destructive" });
            } finally {
                setIsStarting(false);
            }
        },
        [loadRuns, toast],
    );

    const handleStop = useCallback(
        async (run: ScheduledRun) => {
            if (!selectedTask) return;
            setIsStopping(true);
            try {
                await invoke<boolean>("stop_scheduled_task_run", { taskId: selectedTask.id, runId: run.runId });
                toast({ title: "已请求停止", description: "已经产生的外部操作不会撤销" });
            } catch (error) {
                toast({ title: "停止失败", description: errorText(error), variant: "destructive" });
            } finally {
                setIsStopping(false);
            }
        },
        [selectedTask, toast],
    );

    const filteredTasks = useMemo(() => {
        const keyword = search.trim().toLowerCase();
        if (!keyword) return tasks;
        return tasks.filter((task) =>
            [task.name, task.assistantName, task.prompt, task.scheduleText].some((text) => text.toLowerCase().includes(keyword)),
        );
    }, [search, tasks]);

    const grouped = useMemo(
        () => GROUP_ORDER.map((group) => ({ group, items: filteredTasks.filter((task) => taskGroup(task) === group) })).filter((entry) => entry.items.length > 0),
        [filteredTasks],
    );

    const openCreate = useCallback(() => setEditor({ open: true, task: null }), []);

    const addButton = (
        <div className="flex items-center gap-2">
            <Button variant="ghost" size="icon" onClick={() => setIsHelpOpen(true)} title="使用说明">
                <HelpCircle className="h-4 w-4" />
            </Button>
            <Button variant="outline" size="icon" onClick={() => void loadTasks()} title="刷新">
                <RefreshCw className="h-4 w-4" />
            </Button>
            <Button size="icon" onClick={openCreate} title="新建任务">
                <Plus className="h-4 w-4" />
            </Button>
        </div>
    );

    const sidebar = (
        <SidebarList
            title="定时任务"
            description="让助手按时提醒、检查和汇报"
            icon={<Clock className="h-5 w-5" />}
            addButton={addButton}
            searchValue={search}
            onSearchChange={setSearch}
            searchPlaceholder="搜索名称、助手或指令"
        >
            {isLoading ? (
                <div className="flex items-center justify-center py-6 text-sm text-muted-foreground">
                    <RefreshCw className="mr-2 h-3.5 w-3.5 animate-spin" />
                    加载中...
                </div>
            ) : filteredTasks.length === 0 ? (
                <div className="py-6 text-center text-sm text-muted-foreground">{tasks.length ? "没有匹配的任务" : "暂无定时任务"}</div>
            ) : (
                <div className="space-y-4" data-aipp-slot="schedule-task-groups">
                    {grouped.map(({ group, items }) => (
                        <div key={group} className="space-y-1.5">
                            <div className={`px-1 text-[11px] font-medium ${group === "attention" ? "text-destructive" : "text-muted-foreground"}`}>
                                {taskGroupLabels[group]} · {items.length}
                            </div>
                            {items.map((task) => (
                                <TaskListRow key={task.id} task={task} selected={task.id === activeTaskId} onSelect={() => setActiveTaskId(task.id)} />
                            ))}
                        </div>
                    ))}
                </div>
            )}
        </SidebarList>
    );

    const content = selectedTask ? (
        <ScheduleTaskDetail
            task={selectedTask}
            runs={runs}
            selectedRunId={selectedRunId}
            isRunsLoading={isRunsLoading}
            logs={logs}
            isLogsLoading={isLogsLoading}
            isStarting={isStarting}
            isStopping={isStopping}
            onSelectRun={setSelectedRunId}
            onRefreshRuns={() => void loadRuns(selectedTask.id)}
            onToggleEnabled={(enabled) => void handleToggleEnabled(selectedTask, enabled)}
            onEdit={() => setEditor({ open: true, task: selectedTask })}
            onRunNow={() => void handleRunNow(selectedTask)}
            onStop={(run) => void handleStop(run)}
            onDelete={() => setDeleteTarget(selectedTask)}
            onOpenToolLog={setSelectedToolLog}
        />
    ) : (
        <EmptyState
            icon={<Calendar className="h-8 w-8 text-muted-foreground" />}
            title="选择一个任务"
            description="从左侧列表中选择一个定时任务查看详情"
        />
    );

    const toolPayload = selectedToolLog ? parseToolLogPayload(selectedToolLog) : null;

    return (
        <div className="flex h-screen items-center justify-center bg-background" data-aipp-window="schedule" data-aipp-slot="window-root">
            <div className="flex h-screen w-full flex-col overflow-hidden bg-card shadow-none" data-aipp-slot="schedule-main-panel">
                <ConfigPageLayout
                    sidebar={sidebar}
                    content={content}
                    selectOptions={tasks.map((task) => ({ id: String(task.id), label: task.name, icon: <Clock className="h-4 w-4" /> }))}
                    selectedOptionId={activeTaskId ? String(activeTaskId) : undefined}
                    onSelectOption={(id) => setActiveTaskId(Number(id))}
                    selectPlaceholder="选择任务"
                    addButton={addButton}
                    emptyState={
                        <EmptyState
                            icon={<Clock className="h-8 w-8 text-muted-foreground" />}
                            title="还没有定时任务"
                            description="可以在这里新建，也可以直接在聊天里让助手帮你安排，比如「每天早上 9 点提醒我看日程」。"
                            action={
                                <Button size="sm" onClick={openCreate}>
                                    <Plus className="h-4 w-4" />
                                    新建任务
                                </Button>
                            }
                        />
                    }
                    showEmptyState={!isLoading && tasks.length === 0}
                />
            </div>

            <ScheduleTaskEditor
                open={editor.open}
                task={editor.task}
                assistants={assistantOptions}
                saving={isSaving}
                onOpenChange={(open) => setEditor((prev) => ({ open, task: open ? prev.task : null }))}
                onSubmit={(input) => void handleSave(input)}
            />

            <Dialog open={!!selectedToolLog} onOpenChange={(open) => !open && setSelectedToolLog(null)}>
                <DialogContent className="max-h-[80vh] overflow-y-auto sm:max-w-[680px]">
                    <DialogHeader>
                        <DialogTitle className="text-base">工具调用详情</DialogTitle>
                    </DialogHeader>
                    {toolPayload ? (
                        <div className="space-y-3 text-xs">
                            <div className="rounded border bg-muted/20 p-2">
                                <div>
                                    <span className="text-muted-foreground">工具：</span>
                                    {toolPayload.serverName ?? "-"} / {toolPayload.toolName ?? "-"}
                                </div>
                                {"success" in toolPayload && (
                                    <div>
                                        <span className="text-muted-foreground">状态：</span>
                                        {toolPayload.success ? "成功" : "未执行或失败"}
                                    </div>
                                )}
                            </div>
                            <div className="space-y-1">
                                <div className="font-medium">参数</div>
                                <pre className="max-h-44 overflow-auto whitespace-pre-wrap break-all rounded border bg-muted/20 p-2">
                                    {formatToolLogValue(toolPayload.parameters)}
                                </pre>
                            </div>
                            {"result" in toolPayload && (
                                <div className="space-y-1">
                                    <div className="font-medium">返回结果</div>
                                    <pre className="max-h-56 overflow-auto whitespace-pre-wrap break-all rounded border bg-muted/20 p-2">
                                        {formatToolLogValue(toolPayload.result)}
                                    </pre>
                                </div>
                            )}
                        </div>
                    ) : (
                        <pre className="max-h-[60vh] overflow-auto whitespace-pre-wrap break-all rounded border bg-muted/20 p-2 text-xs">
                            {selectedToolLog?.content ?? ""}
                        </pre>
                    )}
                </DialogContent>
            </Dialog>

            <Dialog open={isHelpOpen} onOpenChange={setIsHelpOpen}>
                <DialogContent className="sm:max-w-[520px]">
                    <DialogHeader>
                        <DialogTitle className="text-base">定时任务怎么工作</DialogTitle>
                    </DialogHeader>
                    <div className="space-y-4 text-xs leading-relaxed text-muted-foreground">
                        <p>
                            到点后，执行助手会在一个全新的会话里完成指令，然后把结果以它的身份发给你：默认发到它的管家私聊，也可以让结果单独出现在聊天列表里。
                            助手看不到之前的聊天，所以背景和要求都要写进指令。
                        </p>
                        <p>
                            运行时你不在场：普通的读取和查询直接执行；有副作用的操作由审核模型对照指令判断，超出指令范围的会交给总管家裁决，删除类操作一律需要你本人处理。
                        </p>
                        <p>
                            每次到点都会留下记录：已发送、无新内容、失败、跳过或错过，并写明原因。连续失败 2 次会提醒你，5 次会自动暂停。应用没开时错过的时段默认补跑最近一次。
                        </p>
                        <p>也可以直接在聊天里让助手安排，例如「每周四下午 4 点提醒我今天休息」「工作日每天 9 点帮我看一下 X 有没有更新，有变化再告诉我」。</p>
                    </div>
                    <DialogFooter>
                        <Button size="sm" onClick={() => setIsHelpOpen(false)}>
                            知道了
                        </Button>
                    </DialogFooter>
                </DialogContent>
            </Dialog>

            <ConfirmDialog
                isOpen={!!deleteTarget}
                title="删除任务"
                confirmText={`确认删除定时任务「${deleteTarget?.name ?? ""}」吗？运行记录会一起删除。`}
                onCancel={() => setDeleteTarget(null)}
                onConfirm={async () => {
                    if (!deleteTarget) return;
                    try {
                        await invoke("delete_scheduled_task", { taskId: deleteTarget.id });
                        setDeleteTarget(null);
                        if (activeTaskId === deleteTarget.id) setActiveTaskId(null);
                        toast({ title: "已删除" });
                        await loadTasks();
                    } catch (error) {
                        toast({ title: "删除失败", description: errorText(error), variant: "destructive" });
                    }
                }}
            />
        </div>
    );
}
