import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Plus, Reply, Send, Square, Users, X } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Dialog, DialogContent, DialogHeader, DialogTitle, DialogDescription } from "@/components/ui/dialog";
import { ButlerGroupTasks } from "./ButlerGroupTasks";
import { AskUserQuestionCard } from "@/components/InlineInteractionCards";
import { AcpPermissionDialog, OperationPermissionDialog } from "@/components/OperationPermissionDialog";
import { useAskUserQuestion } from "@/hooks/useInlineInteraction";
import { useAcpPermission, useOperationPermission } from "@/hooks/useOperationPermission";
import { useAcpElicitation } from "@/hooks/useAcpElicitation";
import type { ButlerGroup, ButlerGroupDetail, ButlerGroupMember, ButlerGroupMessage, ButlerGroupPost } from "@/data/ButlerGroup";
import * as groups from "@/services/butlerGroupService";

const statusText: Record<string, string> = { queued: "待送达", prepared: "待启动", delivered: "处理中", answered: "已回应", waiting: "等待中", failed: "失败", cancelled: "已取消", uncertain: "待核对" };

function MemberInteractions({ member }: { member: ButlerGroupMember }) {
    const question = useAskUserQuestion({ conversationId: member.conversation_id });
    const permission = useOperationPermission({ conversationId: member.conversation_id });
    const acp = useAcpPermission({ conversationId: member.conversation_id });
    const elicitation = useAcpElicitation({ conversationId: member.conversation_id });
    return <>
        {!elicitation.questionRequest && elicitation.decisionError && <p role="alert" className="text-sm text-destructive">{member.name}：{elicitation.decisionError}</p>}
        {elicitation.questionRequest && <section aria-label={`${member.name}的原生提问`} className="border-t pt-3">
            <p className="mb-2 text-sm font-medium">{member.name} 正在等你补充信息</p>
            <AskUserQuestionCard request={elicitation.questionRequest} isOpen viewMode="questionnaire"
                errorMessage={elicitation.decisionError} onSubmit={elicitation.handleSubmit} onCancel={elicitation.handleCancel} />
        </section>}
        {question.isDialogOpen && <section aria-label={`${member.name}的问题`} className="border-t pt-3">
            <p className="mb-2 text-sm font-medium">{member.name} 正在等你回答</p>
            <AskUserQuestionCard request={question.pendingRequest} isOpen={question.isDialogOpen}
                viewMode={question.viewMode} completedAnswers={question.completedAnswers} readOnly={question.readOnly}
                onSubmit={question.handleSubmit} onCancel={question.handleCancel} />
        </section>}
        <OperationPermissionDialog request={permission.pendingRequest} isOpen={permission.isDialogOpen}
            isSubmitting={permission.isSubmitting} errorMessage={permission.decisionError} onDecision={permission.handleDecision} />
        <AcpPermissionDialog request={acp.pendingRequest} isOpen={acp.isDialogOpen}
            isSubmitting={acp.isSubmitting} errorMessage={acp.decisionError} onDecision={acp.handleDecision} />
    </>;
}

function GroupRoom({ id, assistants }: { id: number; assistants: { id: number; name: string }[] }) {
    const [detail, setDetail] = useState<ButlerGroupDetail | null>(null);
    const [messages, setMessages] = useState<ButlerGroupMessage[]>([]);
    const [error, setError] = useState("");
    const [body, setBody] = useState("");
    const [reply, setReply] = useState<ButlerGroupMessage | null>(null);
    const [referenceOnly, setReferenceOnly] = useState(false);
    const [recipients, setRecipients] = useState<number[]>([]);
    const [ordered, setOrdered] = useState(false);
    const [independent, setIndependent] = useState(false);
    const [sending, setSending] = useState(false);
    const [invite, setInvite] = useState("");
    const [waitDraft, setWaitDraft] = useState("");
    const [share, setShare] = useState<ButlerGroupMessage | null>(null);
    const [shareMember, setShareMember] = useState("");
    const [goalDraft, setGoalDraft] = useState<string | null>(null);
    const [goalSaving, setGoalSaving] = useState(false);
    const goalRequest = useRef<{ goal: string; key: string } | null>(null);
    const [capabilities, setCapabilities] = useState<groups.GroupCapabilities | null>(null);
    const capabilityVersion = useRef(0);
    const pending = useRef<{ signature: string; post: ButlerGroupPost } | null>(null);
    const mounted = useRef(true);
    const revision = useRef(0);
    const refresh = useCallback(async (propagateError = false) => {
        const version = ++revision.current;
        try {
            const state = await groups.getButlerGroup(id);
            const all: ButlerGroupMessage[] = [];
            let after = 0;
            while (after < state.group.seq) {
                const page = await groups.listButlerGroupMessages(id, after, 200);
                if (!page.length) break;
                all.push(...page.filter(message => message.seq <= state.group.seq));
                after = page[page.length - 1].seq;
            }
            if (mounted.current && version === revision.current) { setDetail(state); setMessages(all); }
        } catch (reason) {
            if (mounted.current && version === revision.current) setError(String(reason));
            if (propagateError) throw reason;
        }
    }, [id]);
    useEffect(() => {
        mounted.current = true;
        void refresh();
        const subscription = groups.onButlerGroupChanged(id, () => void refresh());
        const timer = window.setInterval(() => void refresh(), 5000);
        return () => { mounted.current = false; ++revision.current; clearInterval(timer); void subscription.then(stop => stop()); };
    }, [id, refresh]);
    const act = async (work: () => Promise<unknown>) => {
        setError("");
        try { await work(); if (mounted.current) await refresh(); }
        catch (reason) { if (mounted.current) setError(String(reason)); }
    };
    const send = async () => {
        if (sending || !body.trim()) return;
        const input = { body, reply_to: reply?.seq ?? null, reference_only:referenceOnly, recipients: ordered || independent ? [] : recipients,
            ordered_members: ordered ? recipients : [], independent_members:independent ? recipients : [] };
        if ((ordered || independent) && !recipients.length) { setError("请选择参与讨论的成员；顺序讨论按点击次序发言"); return; }
        const signature = JSON.stringify(input);
        if (pending.current?.signature !== signature) pending.current = { signature, post: { ...input, request_key: crypto.randomUUID() } };
        setSending(true); setError("");
        try {
            await groups.postButlerGroupMessage(id, pending.current.post);
            if (mounted.current) { setBody(""); setReply(null); setReferenceOnly(false); setRecipients([]); pending.current = null; await refresh(); }
        } catch (reason) { if (mounted.current) setError(String(reason)); }
        finally { if (mounted.current) setSending(false); }
    };
    if (!detail) return <div className="p-6">{error || "正在读取协作群…"}</div>;
    const members = detail.members.filter(member => member.active);
    const stopped = detail.group.state === "stopped";
    return <div className="flex min-h-0 flex-1">
        <main className="flex min-w-0 flex-1 flex-col">
            <header className="border-b px-5 py-3"><div className="flex items-center justify-between pr-7"><h2 className="font-semibold">{detail.group.name}</h2><Button variant="ghost" size="sm" disabled={stopped} onClick={() => setGoalDraft(detail.group.goal)}>替换目标</Button></div><p className="mt-1 text-sm text-muted-foreground whitespace-pre-wrap">{detail.group.goal}</p></header>
            <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4" aria-label="群公开消息">
                {!messages.length && <p className="py-8 text-sm text-muted-foreground">发送目标或问题，开始协作。普通消息由总管家接收。</p>}
                {messages.map(message => <article key={message.seq} className="mb-5 max-w-[76ch]" aria-label={`${message.sender_name}的消息`}>
                    <div className="mb-1 flex flex-wrap items-center gap-2 text-sm"><strong>{message.sender_name}</strong>
                        {detail.tasks?.some(task => task.conversation_id === message.sender_conversation) && <span className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">任务：{detail.tasks.find(task => task.conversation_id === message.sender_conversation)?.title}</span>}
                        <time className="text-xs text-muted-foreground">{message.created_at}</time>
                        {!stopped && message.kind !== "notice" && <><Button size="icon" variant="ghost" className="h-6 w-6" title={`回复 ${message.sender_name}`} onClick={() => { setReply(message); setReferenceOnly(false); setRecipients(message.sender_member ? [message.sender_member] : []); }}><Reply className="h-3 w-3" /></Button>
                            <Button size="sm" variant="ghost" className="h-6 text-xs" onClick={() => { setShare(message); setShareMember(""); }}>分享历史</Button></>}
                    </div>
                    {message.reply_to && <p className="mb-1 text-xs text-muted-foreground">{message.reference_only ? "引用资料" : "回复消息"} #{message.reply_to}</p>}
                    <div className={`whitespace-pre-wrap break-words text-sm leading-7 ${message.kind === "notice" || message.kind === "progress" ? "text-muted-foreground" : ""}`}>{message.body}</div>
                </article>)}
                {members.map(member => <MemberInteractions key={member.id} member={member} />)}
                {(detail.tasks ?? []).filter(task => task.state !== "cancelled").map(task => {
                    const member = members.find(value => value.id === task.member_id);
                    return member ? <MemberInteractions key={`task-${task.conversation_id}`} member={{ ...member, conversation_id: task.conversation_id, name: `${member.name} · ${task.title}` }} /> : null;
                })}
            </div>
            {error && <p role="alert" className="border-t px-5 py-2 text-sm text-destructive whitespace-pre-wrap">{error}</p>}
            <footer className="border-t p-4 space-y-2">
                {reply && <div className="flex items-center gap-2 text-xs text-muted-foreground">回复 {reply.sender_name}：{reply.body.slice(0, 90)}<Button variant="ghost" size="icon" className="h-6 w-6" aria-label="取消回复" onClick={() => setReply(null)}><X className="h-3 w-3" /></Button></div>}
                <div className="flex flex-wrap items-center gap-1" aria-label="选择接收成员">
                    {reply && <label className="mr-2 text-xs"><input type="checkbox" checked={referenceOnly} disabled={stopped || sending} onChange={event => { setReferenceOnly(event.target.checked); if (event.target.checked) setRecipients([]); else setRecipients(reply.sender_member ? [reply.sender_member] : []); }} /> 仅引用资料，不要求原作者回应</label>}
                    <label className="mr-2 text-xs"><input type="checkbox" checked={ordered} disabled={stopped || sending} onChange={event => { setOrdered(event.target.checked); if (event.target.checked) setIndependent(false); }} /> 顺序征求意见</label>
                    <label className="mr-2 text-xs"><input type="checkbox" checked={independent} disabled={stopped || sending} onChange={event => { setIndependent(event.target.checked); if (event.target.checked) setOrdered(false); }} /> 各自独立判断</label>
                    {members.map(member => <Button key={member.id} size="sm" variant={recipients.includes(member.id) ? "secondary" : "ghost"} disabled={stopped || sending} aria-pressed={recipients.includes(member.id)} onClick={() => setRecipients(previous => previous.includes(member.id) ? previous.filter(value => value !== member.id) : [...previous, member.id])}>
                        {ordered && recipients.includes(member.id) ? `${recipients.indexOf(member.id) + 1}. ` : ""}{member.name}
                    </Button>)}
                </div>
                <Textarea aria-label="群消息" placeholder={stopped ? "本群工作已停止" : "发送消息；不选择成员时由总管家接收"} value={body} disabled={stopped || sending} onChange={event => setBody(event.target.value)} rows={3} />
                <div className="flex justify-end"><Button disabled={stopped || sending || !body.trim()} onClick={() => void send()}><Send className="mr-2 h-4 w-4" />{sending ? "发送中…" : "发送"}</Button></div>
            </footer>
        </main>
        <aside className="w-56 shrink-0 overflow-y-auto border-l bg-muted/20 p-3 space-y-4" aria-label="群成员与工作状态">
            <ButlerGroupTasks key={id} groupId={id} members={members} messages={messages} tasks={detail.tasks ?? []} stopped={stopped} refresh={() => refresh(true)} />
            <div className="flex items-center justify-between"><h3 className="text-sm font-medium">成员 · {members.length}</h3><Button size="sm" variant="ghost" disabled={stopped} onClick={() => void act(async () => { const results = await groups.stopButlerGroup(id); const failed = results.filter(result => !result.succeeded); if (failed.length) throw new Error(failed.map(result => result.detail).join("\n")); })}><Square className="mr-1 h-3 w-3" />停止全群</Button></div>
            <div className="space-y-1"><label htmlFor={`group-wait-${id}`} className="text-xs text-muted-foreground">发言等待上限（秒）</label><div className="flex gap-1"><Input id={`group-wait-${id}`} type="number" min={5} max={3600} value={waitDraft || String(detail.group.seat_wait_seconds ?? 120)} disabled={stopped} onChange={event=>setWaitDraft(event.target.value)} /><Button size="sm" disabled={stopped || !waitDraft} onClick={()=>void act(async()=>{ await groups.setButlerGroupWaitLimit(id,Number(waitDraft),crypto.randomUUID()); setWaitDraft(""); })}>保存</Button></div><p className="text-xs text-muted-foreground">到期让出席位，工作继续。</p></div>
            {members.map(member => <section key={member.id} className="border-b pb-3">
                <p className="text-sm font-medium">{member.name}</p>
                <Button size="sm" variant="link" className="h-6 p-0 text-xs" onClick={() => { const version=++capabilityVersion.current; void act(async () => { const result=await groups.getButlerGroupMemberCapabilities(id,member.id); if (mounted.current && version===capabilityVersion.current) setCapabilities(result); }); }}>查看本群能力</Button>
                {detail.inbox.filter(item => item.member_id === member.id && (item.conversation_id == null || item.conversation_id === member.conversation_id)).slice(-4).map(item => <p key={item.id} className="mt-1 text-xs text-muted-foreground" title={item.error ?? undefined}>消息 #{item.event_seq} · {statusText[item.state]}{item.error && <span className="block whitespace-pre-wrap">{item.error}</span>}</p>)}
                <div className="mt-2 flex gap-1"><Button size="sm" variant="ghost" disabled={stopped} onClick={() => void act(async () => { const result = await groups.stopButlerGroupMember(id, member.id); if (!result.succeeded) throw new Error(result.detail); })}>停止工作</Button>
                    {member.id !== detail.group.chief_member && <Button size="sm" variant="ghost" disabled={stopped} onClick={() => void act(async () => { const result = await groups.removeButlerGroupMember(id, member.id); if (!result.succeeded) throw new Error(result.detail); })}>移出</Button>}</div>
            </section>)}
            {!stopped && <div className="space-y-2"><select aria-label="邀请助手" className="w-full rounded border bg-background p-2 text-sm" value={invite} onChange={event => setInvite(event.target.value)}><option value="">选择助手</option>{assistants.filter(assistant => !members.some(member => member.assistant_id === assistant.id)).map(assistant => <option key={assistant.id} value={assistant.id}>{assistant.name}</option>)}</select><Button size="sm" disabled={!invite} onClick={() => void act(() => groups.addButlerGroupMember(id, Number(invite)))}>邀请加入</Button></div>}
            {detail.rounds.map(round => <p key={round.id} className="text-xs text-muted-foreground">讨论 #{round.id}：{round.state === "active" ? `第 ${round.next_slot + 1} 位发言` : round.state === "cancelled" ? "已取消" : "席位已结束"}。等待中的工作仍可能继续。</p>)}
            {detail.stop_reports.filter(report => !report.succeeded).map((report, index) => <p key={index} className="text-xs text-destructive">{report.detail}</p>)}
        </aside>
        <Dialog open={goalDraft !== null} onOpenChange={open => { if (!open && !goalSaving) setGoalDraft(null); }}><DialogContent><DialogHeader><DialogTitle>替换群目标</DialogTitle><DialogDescription>停止当前未完成的群内工作，再按新目标继续。旧消息保留；已发生的外部操作不会撤销。</DialogDescription></DialogHeader>
            <Textarea aria-label="新的群目标" value={goalDraft ?? ""} disabled={goalSaving} onChange={event => setGoalDraft(event.target.value)} />
            <Button disabled={goalSaving || !goalDraft?.trim()} onClick={() => void act(async () => {
                if (!goalDraft) return;
                if (goalRequest.current?.goal !== goalDraft) goalRequest.current={goal:goalDraft,key:crypto.randomUUID()};
                setGoalSaving(true);
                try { const reports=await groups.replaceButlerGroupGoal(id,goalDraft,goalRequest.current.key); setGoalDraft(null); const failed=reports.filter(report=>!report.succeeded); if (failed.length) throw new Error(failed.map(report=>report.detail).join("\n")); }
                finally { setGoalSaving(false); }
            })}>{goalSaving ? "正在替换…" : "停止旧工作并替换目标"}</Button>
        </DialogContent></Dialog>
        <Dialog open={capabilities !== null} onOpenChange={open => { if (!open) { ++capabilityVersion.current; setCapabilities(null); } }}><DialogContent className="max-h-[80vh] overflow-auto"><DialogHeader><DialogTitle>{members.find(member=>member.id===capabilities?.member_id)?.name} · 本群能力</DialogTitle><DialogDescription>以下是本群执行会话的工具配置。外部服务连接尚未验证。{capabilities?.dynamic_loading ? "按需发现工具已开启，群协作工具始终直接提供。" : "工具按当前配置提供。"}</DialogDescription></DialogHeader>
            {capabilities?.servers.map(server=><section key={server.id}><h3 className="text-sm font-medium">{server.name}</h3>{server.summary && <p className="text-xs text-muted-foreground">{server.summary}</p>}<ul className="mt-2 space-y-2">{server.tools.filter(tool=>tool.is_enabled).map(tool=><li key={tool.name} className="text-xs"><strong>{tool.name}</strong><p className="mt-1 text-muted-foreground">{tool.description}</p></li>)}</ul></section>)}
        </DialogContent></Dialog>
        <Dialog open={!!share} onOpenChange={open => { if (!open) setShare(null); }}><DialogContent><DialogHeader><DialogTitle>分享这条历史消息</DialogTitle><DialogDescription>只向选定成员分享当前消息，不包含相邻记录或成员的工具过程。</DialogDescription></DialogHeader>
            <p className="max-h-40 overflow-auto text-sm whitespace-pre-wrap">{share?.body}</p><select aria-label="分享给成员" className="rounded border bg-background p-2" value={shareMember} onChange={event => setShareMember(event.target.value)}><option value="">选择接收成员</option>{members.map(member => <option key={member.id} value={member.id}>{member.name}</option>)}</select>
            <Button disabled={!share || !shareMember} onClick={() => void act(async () => { if (!share) return; await groups.shareButlerGroupHistory(id, Number(shareMember), [share.seq], crypto.randomUUID()); setShare(null); })}>分享这一条</Button>
        </DialogContent></Dialog>
    </div>;
}

export default function ButlerGroups({ assistants }: { assistants: { id: number; name: string }[] }) {
    const [items, setItems] = useState<ButlerGroup[]>([]);
    const [selected, setSelected] = useState<number | null>(null);
    const [creating, setCreating] = useState(false);
    const [name, setName] = useState("");
    const [goal, setGoal] = useState("");
    const [invited, setInvited] = useState<number[]>([]);
    const [busy, setBusy] = useState(false);
    const [error, setError] = useState("");
    useEffect(() => {
        let alive = true;
        const load = () => groups.listButlerGroups().then(value => { if (alive) setItems(value); }).catch(reason => { if (alive) setError(String(reason)); });
        void load();
        const subscription = listen("butler_group_changed", () => void load());
        return () => { alive = false; void subscription.then(stop => stop()); };
    }, []);
    const create = async () => {
        setBusy(true); setError("");
        try { const value = await groups.createButlerGroup(name, goal, invited); setCreating(false); setSelected(value.group.id); setName(""); setGoal(""); setInvited([]); }
        catch (reason) { setError(String(reason)); }
        finally { setBusy(false); }
    };
    return <section className="border-t p-2">
        <div className="flex items-center justify-between px-2"><h2 className="text-xs font-medium">协作群</h2><Button variant="ghost" size="icon" className="h-7 w-7" title="新建协作群" onClick={() => setCreating(true)}><Plus className="h-4 w-4" /></Button></div>
        <div className="max-h-52 overflow-auto">{items.map(item => <Button key={item.id} variant="ghost" className="w-full justify-start text-xs" onClick={() => setSelected(item.id)}><Users className="mr-2 h-4 w-4" /><span className="truncate">{item.name}{item.state === "stopped" ? "（已停止）" : ""}</span></Button>)}</div>
        {error && !creating && <p role="alert" className="p-2 text-xs text-destructive">{error}</p>}
        <Dialog open={creating} onOpenChange={setCreating}><DialogContent><DialogHeader><DialogTitle>新建协作群</DialogTitle><DialogDescription>总管家会自动加入。成员在这里使用独立上下文，不带入私聊记录。</DialogDescription></DialogHeader>
            <Input aria-label="群名称" placeholder="群名称" value={name} onChange={event => setName(event.target.value)} /><Textarea aria-label="群目标" placeholder="希望大家一起完成什么？" value={goal} onChange={event => setGoal(event.target.value)} />
            <div className="max-h-56 overflow-auto space-y-2">{assistants.map(assistant => <label key={assistant.id} className="flex items-center gap-2 text-sm"><input type="checkbox" checked={invited.includes(assistant.id)} onChange={event => setInvited(previous => event.target.checked ? [...previous, assistant.id] : previous.filter(id => id !== assistant.id))} />{assistant.name}</label>)}</div>
            {error && <p role="alert" className="text-sm text-destructive">{error}</p>}<Button disabled={busy || !name.trim() || !goal.trim()} onClick={() => void create()}>{busy ? "创建中…" : "创建协作群"}</Button>
        </DialogContent></Dialog>
        <Dialog open={selected !== null} onOpenChange={open => { if (!open) setSelected(null); }}><DialogContent className="flex h-[90vh] w-[92vw] max-w-none flex-col overflow-hidden p-0 sm:max-w-[1200px]"><DialogHeader className="sr-only"><DialogTitle>协作群</DialogTitle><DialogDescription>群公开消息、成员与工作状态</DialogDescription></DialogHeader>{selected !== null && <GroupRoom key={selected} id={selected} assistants={assistants} />}</DialogContent></Dialog>
    </section>;
}
