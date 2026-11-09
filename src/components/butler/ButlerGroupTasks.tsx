import { useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import type { ButlerGroupMember, ButlerGroupMessage, ButlerGroupTask, ButlerGroupTaskRequest } from "@/data/ButlerGroup";
import { startButlerGroupTask, stopButlerGroupTask } from "@/services/butlerGroupService";

const labels: Record<string, string> = { queued: "待送达", prepared: "准备完成", delivered: "处理中", running: "处理中", waiting: "等待中", completed: "已完成", failed: "失败", cancelled: "已停止", uncertain: "待核对" };

export function ButlerGroupTasks({ groupId, members, messages, tasks, stopped, refresh }: {
    groupId: number; members: ButlerGroupMember[]; messages: ButlerGroupMessage[];
    tasks: ButlerGroupTask[]; stopped: boolean; refresh: () => Promise<void>;
}) {
    const [open, setOpen] = useState(false);
    const [member, setMember] = useState("");
    const [title, setTitle] = useState("");
    const [prompt, setPrompt] = useState("");
    const [sources, setSources] = useState<number[]>([]);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    const retry = useRef<{ signature: string; request: ButlerGroupTaskRequest } | null>(null);
    const operation = useRef(false);
    const mounted = useRef(true);
    useEffect(() => {
        mounted.current = true;
        return () => { mounted.current = false; };
    }, []);
    const canCreate = !stopped && !busy && members.some(value => value.active && value.id === Number(member)) && !!title.trim() && !!prompt.trim();
    const create = async () => {
        if (!canCreate || operation.current) return;
        const input = { member_id: Number(member), title, prompt, source_seqs: sources };
        const signature = JSON.stringify(input);
        if (retry.current?.signature !== signature) retry.current = { signature, request: { ...input, request_key: crypto.randomUUID() } };
        operation.current = true;
        setBusy(true); setError("");
        try {
            await startButlerGroupTask(groupId, retry.current.request);
            if (!mounted.current) return;
            retry.current = null; setOpen(false); setTitle(""); setPrompt(""); setSources([]);
            try { await refresh(); }
            catch (reason) { if (mounted.current) setError(`任务已创建，但列表刷新失败：${String(reason)}`); }
        } catch (reason) { if (mounted.current) setError(String(reason)); }
        finally { operation.current = false; if (mounted.current) setBusy(false); }
    };
    const stop = async (conversation: number) => {
        if (operation.current || stopped) return;
        operation.current = true;
        setBusy(true); setError("");
        try {
            const results = await stopButlerGroupTask(groupId, conversation);
            if (!mounted.current) return;
            const failures = results.filter(result => !result.succeeded);
            const diagnostics = failures.map(result => result.detail);
            try { await refresh(); }
            catch (reason) { diagnostics.push(`停止结果已返回，但列表刷新失败：${String(reason)}`); }
            if (mounted.current && diagnostics.length) setError(diagnostics.join("\n"));
        } catch (reason) { if (mounted.current) setError(String(reason)); }
        finally { operation.current = false; if (mounted.current) setBusy(false); }
    };
    return <section aria-label="群内独立任务" className="space-y-2 border-t pt-3">
        <div className="flex items-center justify-between"><h3 className="text-sm font-medium">独立任务</h3><Button size="sm" variant="ghost" disabled={stopped || busy} onClick={() => setOpen(true)}>新建任务</Button></div>
        {!tasks.length && <p className="text-xs text-muted-foreground">需要独立上下文或并行工作时再创建。</p>}
        {tasks.map(task => <article key={task.conversation_id} className="rounded border p-2 space-y-1">
            <p className="text-sm break-words">{task.title}</p>
            <p className="text-xs text-muted-foreground">{members.find(value => value.id === task.member_id)?.name ?? "已退出成员"} · {labels[task.state] ?? task.state}</p>
            <Button size="sm" variant="ghost" disabled={stopped || busy || task.state === "cancelled"} onClick={() => void stop(task.conversation_id)}>停止任务及子任务</Button>
        </article>)}
        {error && !open && <p role="alert" className="whitespace-pre-wrap text-xs text-destructive">{error}</p>}
        <Dialog open={open} onOpenChange={value => { if (!busy) setOpen(value); }}><DialogContent><DialogHeader><DialogTitle>新建群内独立任务</DialogTitle><DialogDescription>执行成员使用单独上下文。只带任务要求与下方明确选中的公开资料，结果回到本群。</DialogDescription></DialogHeader>
            <select aria-label="任务执行成员" className="w-full rounded border bg-background p-2 text-sm" value={member} disabled={busy} onChange={event => setMember(event.target.value)}><option value="">选择执行成员</option>{members.filter(value => value.active).map(value => <option key={value.id} value={value.id}>{value.name}</option>)}</select>
            <Input aria-label="独立任务标题" placeholder="任务标题" value={title} maxLength={200} disabled={busy} onChange={event => setTitle(event.target.value)} />
            <Textarea aria-label="独立任务要求" placeholder="任务要求与交付标准" value={prompt} disabled={busy} onChange={event => setPrompt(event.target.value)} />
            <fieldset className="max-h-48 overflow-auto space-y-2"><legend className="mb-2 text-sm">引用公开资料（可不选）</legend>{messages.map(message => <label key={message.seq} className="flex gap-2 text-xs"><input type="checkbox" disabled={busy || (!sources.includes(message.seq) && sources.length >= 100)} checked={sources.includes(message.seq)} onChange={event => setSources(previous => event.target.checked ? [...previous, message.seq] : previous.filter(seq => seq !== message.seq))} /><span>#{message.seq} {message.sender_name}：{message.body.slice(0, 80)}</span></label>)}</fieldset>
            {error && <p role="alert" className="text-sm text-destructive">{error}</p>}
            <Button disabled={!canCreate} onClick={() => void create()}>{busy ? "创建中…" : "创建独立任务"}</Button>
        </DialogContent></Dialog>
    </section>;
}
