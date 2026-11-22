import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useFilteredModels } from "@/hooks/useFilteredModels";
import { center, changeItem, showConversation, type Center, type ProactiveConfig } from "./proactiveApi";

const switches = [
    ["enabled", "启用主动整理"], ["private_chats", "总管家与联系人私聊"], ["groups", "管家群聊"],
    ["normal_chats", "普通聊天"], ["remember", "整理长期记忆"], ["research", "提前查资料、准备简报"], ["reminders", "主动提醒与祝福"],
    ["adaptive_window", "按近两周活跃时间调整整理和送达"],
] as const;
const times = [["digest", "每日复盘"], ["quiet_start", "免打扰开始"], ["quiet_end", "免打扰结束"], ["release", "夜间提醒送达"]] as const;
const statuses: Record<string, string> = { active: "跟进中", dismissed: "已关闭", done: "已完成", acted: "已整理", quiet: "无新事项",
    failed: "失败", partial: "本批已整理，待续扫", running: "整理中", digest: "深度复盘", manual: "手动整理", pulse: "定期巡检", idle_after_chat: "聊后整理",
    wake:"事项重新预研",schedule:"事项排期",finding:"整理事项",milestone:"事项唤醒",delivery:"提醒投递", proposal:"建议派发", feishu:"飞书投递", queued:"等待送达", delivered:"已送达", acknowledged:"已确认",cancelled:"已取消",sent:"已发送" };
export default function ButlerProactiveSettings() {
    const [settings, setSettings] = useState<ProactiveConfig>();
    const [content, setContent] = useState<Center>();
    const [status, setStatus] = useState("");
    const [busy, setBusy] = useState(false);
    const [edit, setEdit] = useState<{ id: number; title: string; date:string; yearly:boolean; calendar:string; lead:number }>();
    const { models } = useFilteredModels(0);
    useEffect(() => { invoke<ProactiveConfig>("get_butler_proactive_config").then(setSettings).catch(e => setStatus(String(e)));
        center().then(setContent).catch(e => setStatus(String(e))); }, []);
    async function work(fn: () => Promise<string | void>) {
        setBusy(true);
        try { setStatus((await fn()) || "已保存"); setContent(await center()); }
        catch (e) { setStatus(String(e)); }
        finally { setBusy(false); }
    }
    if (!settings) return <p role="status">{status || "正在读取主动整理设置…"}</p>;
    const set = <K extends keyof ProactiveConfig>(key: K, value: ProactiveConfig[K]) => setSettings({ ...settings, [key]: value });
    const save = () => invoke<void>("save_butler_proactive_config", { settings });
    return <section className="my-4 space-y-3 rounded-lg border p-4">
        <h3 className="font-medium">主动整理与关怀</h3>
        <Tabs defaultValue="settings" onValueChange={tab => { if (tab !== "settings") void center().then(setContent).catch(e => setStatus(String(e))); }}>
            <TabsList><TabsTrigger value="settings">设置</TabsTrigger><TabsTrigger value="items">纪念日与待跟进</TabsTrigger><TabsTrigger value="runs">整理手记</TabsTrigger></TabsList>
            <TabsContent value="settings" className="space-y-4">
                <div className="grid gap-2">{switches.map(([key, label]) => <label key={key} className="flex items-center justify-between gap-2 text-sm">
                    {label}<Switch checked={settings[key]} onCheckedChange={v => set(key, v)} /></label>)}</div>
                <label className="flex items-center justify-between text-sm">提醒节律
                    <select className="rounded border bg-background p-1" value={settings.pulse_hours===3 && settings.daily_limit===3 ? "gentle" : settings.pulse_hours===1 && settings.daily_limit===6 ? "active" : "custom"}
                        onChange={e => {if(e.target.value!=="custom") setSettings({...settings,pulse_hours:e.target.value==="active"?1:3,daily_limit:e.target.value==="active"?6:3});}}>
                        <option value="gentle">温和</option><option value="active">积极</option><option value="custom" disabled>自定义</option>
                    </select></label>
                <div className="grid grid-cols-2 gap-3">{times.map(([key, label]) => <label key={key} className="text-xs">{label}
                    <Input type="time" value={settings[key]} onChange={e => set(key, e.target.value)} /></label>)}
                    <label className="text-xs">巡检间隔（小时）<Input type="number" min={1} max={24} value={settings.pulse_hours} onChange={e => set("pulse_hours", Number(e.target.value))} /></label>
                    <label className="text-xs">每日触达上限<Input type="number" min={0} max={100} value={settings.daily_limit} onChange={e => set("daily_limit", Number(e.target.value))} /></label>
                    {(["small_model","deep_model"] as const).map(key => <label className="grid gap-1 text-xs" key={key}>{key === "small_model" ? "初筛模型" : "整理模型"}
                        <select className="h-9 rounded-md border bg-background" value={settings[key]} onChange={e => set(key,e.target.value)}>
                            <option value="">{key === "small_model" ? "沿用管家辅助模型设置" : "沿用总管家模型"}</option>
                            {models.map(m => <option key={m.id} value={m.code + "%%" + m.llm_provider_id}>{m.name}</option>)}
                        </select></label>)}
                </div>
                <div className="flex gap-2"><Button disabled={busy} onClick={() => void work(save)}>保存主动整理设置</Button>
                    <Button variant="outline" disabled={busy || !settings.enabled} onClick={() => void work(async () => { await save(); return invoke<string>("run_butler_proactive_now"); })}>立即整理一次</Button></div>
                <label className="block text-sm">飞书提醒接收人（Open ID，留空关闭）<Input value={settings.feishu_open_id} onChange={e => set("feishu_open_id",e.target.value.trim())} /></label>
                {content?.window && <Button size="sm" variant="outline" onClick={() => setSettings({...settings,release:content.window!.release,digest:content.window!.digest})}>
                    采用近两周活跃时间：{content.window.release} 送达、{content.window.digest} 整理</Button>}
            </TabsContent>
            <TabsContent value="items">{content?.items.length === 0 && <p>暂无跟进事项</p>}
                {content?.items.map(i => <article key={i.id} className="space-y-1 border-b py-2 text-sm">
                    {edit?.id === i.id ? <div className="grid gap-2">
                        <Input aria-label="事项标题" value={edit.title} onChange={e => setEdit({...edit,title:e.target.value})} />
                        <label>提醒日期<Input value={edit.date} placeholder={edit.yearly?"MM-DD（闰月 LMM-DD）":"YYYY-MM-DD"} onChange={e=>setEdit({...edit,date:e.target.value})}/></label>
                        <label>日历 <select value={edit.calendar} onChange={e=>setEdit({...edit,calendar:e.target.value})}><option value="solar">公历</option><option value="lunar">农历</option></select></label>
                        <label>每年重复 <Switch checked={edit.yearly} onCheckedChange={yearly=>setEdit({...edit,yearly})}/></label>
                        <label>提前天数<Input type="number" min={0} max={366} value={edit.lead} onChange={e=>setEdit({...edit,lead:Number(e.target.value)})}/></label>
                        <div><Button disabled={busy} onClick={() => void work(async () => {await changeItem(i.id,i.revision,"rename",undefined,edit.title,
                            {date:edit.date,yearly:edit.yearly,calendar:edit.calendar,lead:edit.lead});setEdit(undefined);})}>保存</Button>
                            <Button variant="ghost" onClick={()=>setEdit(undefined)}>取消</Button></div></div>
                        : <p>{i.title} <span className="text-xs text-muted-foreground">{statuses[i.status] || i.status} {i.date}</span></p>}
                    {i.tracking && <p className="whitespace-pre-wrap text-muted-foreground">{i.tracking}</p>}
                    {i.detail && <details><summary className="cursor-pointer">{i.kind==="prep_topic"?"查看预研简报":"事项详情"}</summary>
                        <p className="mt-1 whitespace-pre-wrap">{i.detail}</p></details>}
                    <div className="flex gap-2"><Button size="sm" variant="ghost" onClick={() => showConversation(i.source,i.position)}>查看原话</Button>
                        {i.status === "active" && <><Button size="sm" variant="ghost" onClick={() => setEdit({...i,date:i.date??""})}>编辑</Button>
                        <Button size="sm" variant="ghost" disabled={busy} onClick={() => void work(() => changeItem(i.id,i.revision,"dismiss"))}>不再跟进</Button></>}</div>
                </article>)}</TabsContent>
            <TabsContent value="runs">{content?.runs.length === 0 && <p>暂无整理记录</p>}{content?.runs.map(r => <article key={r.id} className="border-b py-2 text-sm">
                <p>{new Date(r.time * 1000).toLocaleString()} · {statuses[r.trigger] || r.trigger} · {statuses[r.status] || r.status}</p>
                <p className="text-muted-foreground">{r.summary}</p>{r.conversation && <Button size="sm" variant="ghost" onClick={() => showConversation(r.conversation!)}>查看过程</Button>}
            </article>)}
                {!!content?.deliveries.length && <h4 className="mt-4 font-medium">近期投递</h4>}
                {content?.deliveries.map((d,index)=><article className="border-b py-2 text-sm" key={index}>
                    <p>{d.date} · {d.title} · {statuses[d.status]??d.status}</p>
                    {d.fallback && <p>总管家代送原因：{d.fallback}</p>}
                    {d.external && <p>飞书：{statuses[d.external]??d.external} {d.error}</p>}
                </article>)}</TabsContent>
        </Tabs>
        {status && <p role="status" className="text-sm">{status}</p>}
    </section>;
}
