import { useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDataRefresh } from "@/hooks/useDataRefresh";
import { useMobileResume } from "@/hooks/useMobileResume";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import ConfigPageLoading from "@/components/common/ConfigPageLoading";
import {
    butlerTaskStatusLabel,
    type ButlerTaskDetailResponse,
    type ButlerTaskListItem,
} from "@/data/Butler";
import type { OperationPermissionRequest } from "@/components/OperationPermissionDialog";

interface Overview {
    main_conversation_id: number | null;
    tasks: ButlerTaskListItem[];
    local_permissions: { event: OperationPermissionRequest }[];
}
interface Schedule { id: number; name: string; assistantId: number; nextRunAt?: string; taskPrompt: string; isEnabled: boolean }
export interface MobileTaskRun { runId: string; status: string; localRunning: boolean; summary?: string; errorMessage?: string }
interface FeishuStatus { running: boolean; connected: boolean; configured: boolean; status_text: string; last_error?: string; last_event_at?: string; status_detail?: string }
interface Assistant { id: number; name: string; assistant_type?: number }

export function mobileRunLabel(run: MobileTaskRun): string {
    if (run.status === "running" && run.localRunning) return "本机执行中";
    if (run.status === "running") return "结果未知（本机没有运行实例）";
    if (run.status === "missed") return "已错过，未执行";
    return `历史记录：${run.status}`;
}

export default function MobileTasksConfig() {
    const [overview, setOverview] = useState<Overview>();
    const [schedules, setSchedules] = useState<Schedule[]>([]);
    const [assistants, setAssistants] = useState<Assistant[]>([]);
    const [feishu, setFeishu] = useState<FeishuStatus>();
    const [errors, setErrors] = useState<string[]>([]);
    const [actionError, setActionError] = useState<string>();
    const [busy, setBusy] = useState(false);
    const activeActions = useRef(new Set<string>());
    const [title, setTitle] = useState("");
    const [goal, setGoal] = useState("");
    const [assistantId, setAssistantId] = useState("");
    const [detail, setDetail] = useState<{ taskId: number; data: ButlerTaskDetailResponse }>();
    const [selectedSchedule, setSelectedSchedule] = useState<number>();
    const [runs, setRuns] = useState<MobileTaskRun[]>([]);
    // 首次读取完成前不渲染「暂无…」等空数据界面
    const [loaded, setLoaded] = useState(false);
    const selectedScheduleRef = useRef<number | undefined>(undefined);
    const runReadRevision = useRef(0);
    const readRuns = async (taskId: number, isCurrent = () => true) => {
        const revision = ++runReadRevision.current;
        const current = () => isCurrent() && revision === runReadRevision.current && selectedScheduleRef.current === taskId;
        try {
            const result = await invoke<{ runs: MobileTaskRun[] }>("list_scheduled_task_runs", { taskId, limit: 30 });
            if (current()) setRuns(result.runs);
        } catch (error) {
            if (current()) { setRuns([]); throw error; }
        }
    };
    const refresh = useDataRefresh({
        initial: true,
        events: ["butler_task_created", "butler_task_updated", "butler_task_permission_state_changed", "operation-permission-request", "operation-permission-resolved", "scheduled_task_run_updated", "scheduled_task_run_created", "butler_feishu_status_changed"],
        refresh: async isCurrent => {
            const results = await Promise.allSettled([
                invoke<Overview>("get_mobile_butler_overview"), invoke<Schedule[]>("list_scheduled_tasks"),
                invoke<FeishuStatus>("get_butler_feishu_runtime_status"), invoke<Assistant[]>("get_assistants"),
            ]);
            if (!isCurrent()) return;
            const [butler, schedule, connection, available] = results;
            if (butler.status === "fulfilled") setOverview(butler.value); else setOverview(undefined);
            if (schedule.status === "fulfilled") setSchedules(schedule.value); else setSchedules([]);
            if (connection.status === "fulfilled") setFeishu(connection.value); else setFeishu(undefined);
            if (available.status === "fulfilled") setAssistants(available.value); else setAssistants([]);
            setErrors(results.flatMap((result, index) => result.status === "rejected" ? [`${["Butler", "定时任务", "飞书", "助手"][index]}读取失败：${String(result.reason)}`] : []));
            setLoaded(true);
            if (selectedScheduleRef.current !== undefined) await readRuns(selectedScheduleRef.current, isCurrent);
        },
        onError: error => { setErrors([String(error)]); setLoaded(true); },
    });
    useMobileResume(refresh);
    const act = async (label: string, action: () => Promise<unknown>, allowWhileBusy = false) => {
        if (activeActions.current.has(label) || (!allowWhileBusy && activeActions.current.size)) return;
        activeActions.current.add(label); setBusy(true);
        try { await action(); setActionError(undefined); }
        catch (error) { setActionError(`${label}失败：${String(error)}`); }
        finally { activeActions.current.delete(label); setBusy(activeActions.current.size > 0); refresh(); }
    };
    if (!loaded) return <ConfigPageLoading />;
    return <div className="h-full overflow-auto p-3 safe-bottom space-y-5">
        <h1 className="text-lg font-semibold">本机任务</h1>
        <p className="text-sm text-muted-foreground">任务和飞书连接依赖应用进程。锁屏或退出后不保证继续执行。历史记录仅供查看，不代表本机正在运行。</p>
        <Button className="min-h-11" variant="outline" onClick={refresh}>刷新本机状态</Button>
        {errors.map(error => <p role="alert" className="break-all text-sm" key={error}>{error}</p>)}
        {actionError && <p role="alert" className="break-all text-sm">{actionError}</p>}
        <section className="space-y-3"><h2 className="text-sm font-medium">Butler 任务</h2>
            {overview?.main_conversation_id == null ? <Button disabled={busy || !overview} className="min-h-11" onClick={() => void act("初始化 Butler", () => invoke("load_butler_main_conversation", { reconcile: false }))}>初始化本机任务入口</Button> : <>
                <label className="block">执行助手<select className="block w-full min-h-11 border rounded bg-background" value={assistantId} onChange={event => setAssistantId(event.target.value)}><option value="">请选择助手</option>{assistants.filter(a => !a.assistant_type).map(a => <option key={a.id} value={a.id}>{a.name}</option>)}</select></label>
                <Input aria-label="任务标题" value={title} onChange={event => setTitle(event.target.value)} placeholder="任务标题" />
                <Textarea aria-label="任务目标" value={goal} onChange={event => setGoal(event.target.value)} placeholder="任务目标" />
                <Button className="min-h-11" disabled={busy || !title.trim() || !goal.trim() || !assistantId} onClick={() => void act("派发任务", async () => {
                    await invoke("spawn_butler_task_conversation", { request: { butler_conversation_id: overview.main_conversation_id, title, goal, executor_assistant_id: Number(assistantId) } });
                    setTitle(""); setGoal("");
                })}>在本机派发</Button>
            </>}
            {overview?.tasks.map(task => <article key={task.task_conversation_id} className="border rounded p-3 space-y-2">
                <h3>{task.title}</h3><p className="text-sm">{task.is_running ? "本机执行中" : `历史记录：${butlerTaskStatusLabel(task.status)}；本机未运行`}</p>
                <p className="whitespace-pre-wrap break-words">{task.last_summary}</p>
                <Button className="min-h-11" variant="outline" disabled={busy} onClick={() => void act("读取任务", async () => setDetail({ taskId: task.task_conversation_id, data: await invoke<ButlerTaskDetailResponse>("get_butler_task_detail", { taskConversationId: task.task_conversation_id }) }))}>查看详情</Button>
                {task.is_running && <Button className="min-h-11" variant="outline" onClick={() => void act("取消本机任务", () => invoke("cancel_ai", { conversationId: task.task_conversation_id }), true)}>取消本机执行</Button>}
                {/* 详情紧跟所点任务展示，避免长列表时出现在页面底部 */}
                {detail?.taskId === task.task_conversation_id && <div className="space-y-2 border-t pt-2"><h4 className="text-sm font-medium">{detail.data.task.title}</h4><p className="whitespace-pre-wrap break-words">{detail.data.definition.goal}</p><p className="whitespace-pre-wrap break-words">{detail.data.result?.summary ?? "暂无完成结果"}</p><Button className="min-h-11" variant="ghost" onClick={() => setDetail(undefined)}>关闭详情</Button></div>}
            </article>)}
            {overview && overview.main_conversation_id != null && overview.tasks.length === 0 && <p className="text-sm text-muted-foreground">暂无 Butler 任务</p>}
            {overview?.local_permissions.map(({ event }) => <article className="border rounded p-3" key={event.request_id}><h3>本机待审批：{event.operation}</h3><p className="break-all">{event.path}</p><p className="text-xs break-all">请求 {event.request_id}</p>{(["allow", "deny"] as const).map(decision => <Button className="min-h-11" key={decision} variant="outline" onClick={() => void act("提交本机审批", () => invoke("confirm_operation_permission", { requestId: event.request_id, decision }), true)}>{decision === "allow" ? "允许一次" : "拒绝"}</Button>)}</article>)}
        </section>
        <section className="space-y-3"><h2 className="text-sm font-medium">定时任务</h2><p className="text-sm text-muted-foreground">自动触发依赖本机进程；应用启动前错过的触发不会自动补跑；进程仍存活时由系统决定能否继续调度。手动运行会在本机执行所选任务。</p>
            {schedules.map(task => <article className="border rounded p-3 space-y-2" key={task.id}><h3>{task.name}</h3><p className="text-sm">{task.isEnabled ? "已启用" : "已停用"} · 下次：{task.nextRunAt ?? "无"}</p><p className="whitespace-pre-wrap break-words">{task.taskPrompt}</p>
                <Button className="min-h-11" variant="outline" disabled={busy} onClick={() => void act("读取运行记录", async () => { selectedScheduleRef.current = task.id; setSelectedSchedule(task.id); setRuns([]); await readRuns(task.id); })}>查看运行记录</Button>
                <Button className="min-h-11" disabled={busy} onClick={() => void act("手动运行", async () => { selectedScheduleRef.current = task.id; setSelectedSchedule(task.id); setRuns([]); const pending = invoke<{ success: boolean; error?: string }>("run_scheduled_task_now", { taskId: task.id }); refresh(); const result = await pending; if (!result.success) throw new Error(result.error ?? `task_id=${task.id}; 执行未返回失败原因`); })}>在本机运行一次</Button>
                {selectedSchedule === task.id && runs.map(run => <div key={run.runId} className="border-t pt-2"><p>{mobileRunLabel(run)}</p><p className="text-xs break-all">{run.runId}</p><p className="whitespace-pre-wrap break-words">{run.errorMessage ?? run.summary}</p>{run.status === "running" && run.localRunning && <Button className="min-h-11" variant="outline" onClick={() => void act("停止本机运行", () => invoke("stop_scheduled_task_run", { taskId: task.id, runId: run.runId }), true)}>停止本机运行</Button>}</div>)}
            </article>)}
            {!schedules.length && <p className="text-sm text-muted-foreground">暂无定时任务</p>}
        </section>
        <section className="space-y-3"><h2 className="text-sm font-medium">飞书本机连接</h2>{feishu && <><p>{feishu.status_text}</p><p className="text-sm">{feishu.connected ? "本机已连接" : feishu.running ? "连接中，等待事件确认" : "本机未连接"} · 最后事件：{feishu.last_event_at ?? "暂无"}</p><p className="text-sm break-words">{feishu.last_error}</p><p className="text-sm">{feishu.status_detail}</p><Button className="min-h-11" disabled={busy || !feishu.configured} onClick={() => void act("启动本机飞书连接", () => invoke("refresh_butler_feishu_runtime_command"))}>按配置连接</Button><Button className="min-h-11" variant="outline" disabled={busy || !feishu.running} onClick={() => void act("停止本机飞书连接", () => invoke("stop_mobile_feishu_runtime"))}>停止本机连接</Button></>}</section>
    </div>;
}
