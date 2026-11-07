import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import type { ButlerTaskListItem } from "@/data/Butler";
import { Button } from "@/components/ui/button";

interface Control { paused: boolean; delegation_stopped: boolean; cancelled: boolean; note: string }
export default function ButlerProgressCard({ task }: { task: ButlerTaskListItem }) {
    const [control, setControl] = useState<Control>();
    const [error, setError] = useState("");
    const [busy, setBusy] = useState(false);
    const id = task.task_conversation_id;
    const currentId = useRef(id); currentId.current = id;
    useEffect(() => {
        let live = true; setControl(undefined); setError(""); setBusy(false);
        void invoke<Control>("get_butler_task_control", {taskConversationId: id})
            .then(value => {if (live) setControl(value);}).catch(e => {if (live) setError(String(e));});
        return () => {live = false;};
    }, [id,task.status]);
    const act = async (action: string) => {
        if (busy) return;
        setBusy(true); setError("");
        try { const value=await invoke<Control>("control_butler_task",{taskConversationId:id,action}); if(currentId.current===id) setControl(value); }
        catch(e) {if(currentId.current===id) setError(String(e));}
        finally {if(currentId.current===id) setBusy(false);}
    };
    return <section className="space-y-2 border rounded p-3 text-xs" aria-label={`工作进展：${task.title}`}>
        <div className="font-medium">{task.title}</div>
        <div>执行助手：{task.executor_assistant_name}</div>
        {task.progress && <>
            {!!task.progress.waiting_for.length && <div>在等：{task.progress.waiting_for.join("、")}</div>}
            {task.progress.blocker && <div className="text-destructive">{task.progress.blocker}</div>}
            <div>下一步：{task.progress.next_step}</div>
            {task.progress.evidence.map((e,i)=><div key={i}>{e.label ?? "证据"}：本任务消息 #{e.message_id}</div>)}
            {task.progress.artifacts.map((a,i)=><div className="break-all" key={i}>{a.label ?? "成果"}：{a.location ?? "会话内附件"}</div>)}
        </>}
        {task.last_summary && <p className="whitespace-pre-wrap break-words">{task.last_summary}</p>}
        {control?.note && <p>{control.note}</p>}
        {!task.is_finalized && !control?.cancelled && <div className="flex flex-wrap gap-1">
            <Button className="min-h-11" size="sm" variant="outline" disabled={busy} onClick={()=>void act(control?.paused?"resume":"pause")}>{control?.paused?"恢复接收请求":"暂停后续执行"}</Button>
            <Button className="min-h-11" size="sm" variant="outline" disabled={busy||control?.delegation_stopped} onClick={()=>void act("stop_delegation")}>停止新分工</Button>
            <Button className="min-h-11" size="sm" variant="outline" disabled={busy} onClick={()=>void act("cancel")}>停止整项工作</Button>
        </div>}
        {error && <p role="alert" className="text-destructive">{error}</p>}
    </section>;
}
