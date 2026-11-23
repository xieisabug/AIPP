import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { Loader2 } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Switch } from "@/components/ui/switch";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { getErrorMessage } from "@/utils/error";
import { center, changeItem, showConversation, type Center, type ProactiveItem } from "./proactiveApi";

const labels: Record<string, string> = {
    active: "跟进中", dismissed: "已关闭", done: "已完成", acted: "已整理", quiet: "无新事项", failed: "失败",
    partial: "本批已整理，待续扫", running: "整理中", digest: "深度复盘", manual: "手动整理", pulse: "定期巡检",
    idle_after_chat: "聊后整理", wake: "事项重新预研", schedule: "事项排期", finding: "整理事项", milestone: "事项唤醒",
    delivery: "提醒投递", proposal: "建议派发", feishu: "飞书投递", queued: "等待送达", delivered: "已送达",
    acknowledged: "已确认", cancelled: "已取消", sent: "已发送",
};
const label = (key: string) => labels[key] ?? key;

const listClassName = "divide-y rounded-lg border";
const rowClassName = "space-y-1.5 px-4 py-3 text-sm";

type Draft = { title: string; date: string; yearly: boolean; calendar: string; lead: number };

function itemTiming(i: ProactiveItem) {
    const parts = [i.date, i.calendar === "lunar" ? "农历" : null, i.yearly ? "每年" : null, i.lead > 0 ? `提前 ${i.lead} 天提醒` : null];
    return parts.filter(Boolean).join(" · ");
}

function ItemEditor({ item, busy, onSave, onCancel }: {
    item: ProactiveItem; busy: boolean; onSave: (draft: Draft) => void; onCancel: () => void;
}) {
    const [draft, setDraft] = useState<Draft>({ title: item.title, date: item.date ?? "", yearly: item.yearly, calendar: item.calendar, lead: item.lead });
    return <div className="grid gap-3">
        <Input aria-label="事项标题" value={draft.title} onChange={e => setDraft({ ...draft, title: e.target.value })} />
        <div className="flex flex-wrap items-center gap-3">
            <Input aria-label="提醒日期" className="w-44" value={draft.date}
                placeholder={draft.yearly ? "MM-DD（闰月 LMM-DD）" : "YYYY-MM-DD"} onChange={e => setDraft({ ...draft, date: e.target.value })} />
            <Select value={draft.calendar} onValueChange={calendar => setDraft({ ...draft, calendar })}>
                <SelectTrigger aria-label="日历" className="w-24"><SelectValue /></SelectTrigger>
                <SelectContent><SelectItem value="solar">公历</SelectItem><SelectItem value="lunar">农历</SelectItem></SelectContent>
            </Select>
            <label className="flex items-center gap-2 text-muted-foreground">
                每年重复<Switch checked={draft.yearly} onCheckedChange={yearly => setDraft({ ...draft, yearly })} />
            </label>
            <label className="flex items-center gap-2 text-muted-foreground">
                提前<Input type="number" aria-label="提前天数" className="h-8 w-16" min={0} max={366} value={draft.lead}
                    onChange={e => setDraft({ ...draft, lead: Number(e.target.value) })} />天
            </label>
        </div>
        <div className="flex gap-2">
            <Button size="sm" disabled={busy} onClick={() => onSave(draft)}>保存</Button>
            <Button size="sm" variant="ghost" onClick={onCancel}>取消</Button>
        </div>
    </div>;
}

/** 主动整理的运行内容：正在跟进的事项、整理手记与投递记录。设置在总管家设置弹窗里。 */
export default function ButlerProactiveCenter({ open, onOpenChange, enabled }: {
    open: boolean;
    onOpenChange: (open: boolean) => void;
    /** 已保存的开启状态；关闭时不能立即整理 */
    enabled: boolean;
}) {
    const [content, setContent] = useState<Center>();
    const [loadError, setLoadError] = useState<string>();
    const [busy, setBusy] = useState(false);
    const [running, setRunning] = useState(false);
    const [editing, setEditing] = useState<number>();

    const reload = useCallback(() => center()
        .then(next => { setContent(next); setLoadError(undefined); })
        .catch(e => setLoadError(`读取主动整理内容失败：${getErrorMessage(e)}`)), []);

    useEffect(() => {
        if (!open) return;
        void reload();
        const unlisten = listen("butler_proactive_changed", () => void reload());
        return () => { void unlisten.then(stop => stop()).catch(console.warn); };
    }, [open, reload]);

    async function work(fn: () => Promise<unknown>, done = "已保存") {
        setBusy(true);
        try { await fn(); toast.success(done); await reload(); }
        catch (e) { toast.error(getErrorMessage(e)); }
        finally { setBusy(false); }
    }

    async function runNow() {
        setRunning(true);
        try { toast.success(await invoke<string>("run_butler_proactive_now") || "整理完成"); await reload(); }
        catch (e) { toast.error(`整理失败：${getErrorMessage(e)}`); }
        finally { setRunning(false); }
    }

    const active = content?.items.filter(i => i.status === "active") ?? [];
    const closed = content?.items.filter(i => i.status !== "active") ?? [];

    const renderItem = (i: ProactiveItem) => <article key={i.id} className={rowClassName} data-aipp-slot="butler-proactive-item">
        {editing === i.id
            ? <ItemEditor item={i} busy={busy} onCancel={() => setEditing(undefined)} onSave={draft => void work(async () => {
                await changeItem(i.id, i.revision, "rename", undefined, draft.title,
                    { date: draft.date, yearly: draft.yearly, calendar: draft.calendar, lead: draft.lead });
                setEditing(undefined);
            })} />
            : <div className="flex items-start justify-between gap-3">
                <div className="min-w-0 space-y-0.5">
                    <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">{i.title}</span>
                        {i.status !== "active" && <Badge variant="outline" className="font-normal">{label(i.status)}</Badge>}
                    </div>
                    {itemTiming(i) && <p className="text-xs text-muted-foreground">{itemTiming(i)}</p>}
                </div>
                <div className="flex shrink-0 items-center">
                    <Button size="sm" variant="ghost" onClick={() => showConversation(i.source, i.position)}>查看原话</Button>
                    {i.status === "active" && <>
                        <Button size="sm" variant="ghost" onClick={() => setEditing(i.id)}>编辑</Button>
                        <Button size="sm" variant="ghost" disabled={busy}
                            onClick={() => void work(() => changeItem(i.id, i.revision, "dismiss"), "已不再跟进")}>不再跟进</Button>
                    </>}
                </div>
            </div>}
        {i.tracking && <p className="whitespace-pre-wrap text-muted-foreground">{i.tracking}</p>}
        {i.detail && <details className="group">
            <summary className="cursor-pointer text-muted-foreground hover:text-foreground">{i.kind === "prep_topic" ? "查看预研简报" : "事项详情"}</summary>
            <p className="mt-1.5 whitespace-pre-wrap rounded-md bg-muted/40 p-3">{i.detail}</p>
        </details>}
    </article>;

    return <Dialog open={open} onOpenChange={onOpenChange}>
        <DialogContent className="flex max-h-[85vh] flex-col gap-0 overflow-hidden p-0 sm:max-w-2xl" data-aipp-slot="butler-proactive-center">
            <DialogHeader className="border-b px-6 py-4">
                <DialogTitle>主动整理</DialogTitle>
                <DialogDescription>管家正在跟进的事项，以及每次整理的记录。节律与范围在总管家设置中调整。</DialogDescription>
            </DialogHeader>
            <Tabs defaultValue="items" className="flex min-h-0 flex-1 flex-col gap-0">
                <div className="flex items-center justify-between gap-3 px-6 pt-4">
                    <TabsList>
                        <TabsTrigger value="items">跟进事项{active.length > 0 ? ` ${active.length}` : ""}</TabsTrigger>
                        <TabsTrigger value="runs">整理手记</TabsTrigger>
                    </TabsList>
                    <Button size="sm" variant="outline" disabled={!enabled || running} onClick={() => void runNow()}
                        title={enabled ? "立即回顾一次已开启范围内的聊天" : "主动整理未开启"}>
                        {running && <Loader2 className="h-4 w-4 mr-1.5 animate-spin" />}立即整理一次
                    </Button>
                </div>
                <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
                    {loadError && <p role="alert" className="mb-3 text-sm text-destructive">{loadError}</p>}
                    {!content && !loadError && <p className="text-sm text-muted-foreground">正在读取…</p>}
                    {content && <>
                        <TabsContent value="items" className="mt-0 space-y-4">
                            {active.length > 0
                                ? <div className={listClassName}>{active.map(renderItem)}</div>
                                : <p className="rounded-lg border px-4 py-6 text-center text-sm text-muted-foreground">
                                    暂无跟进中的事项。管家复盘聊天时，会把纪念日和需要跟进的事情记在这里。
                                </p>}
                            {closed.length > 0 && <details className="group">
                                <summary className="cursor-pointer text-sm text-muted-foreground hover:text-foreground">已完成或关闭（{closed.length}）</summary>
                                <div className={`mt-2 ${listClassName}`}>{closed.map(renderItem)}</div>
                            </details>}
                        </TabsContent>
                        <TabsContent value="runs" className="mt-0 space-y-4">
                            {content.runs.length > 0
                                ? <div className={listClassName}>{content.runs.map(r => <article key={r.id} className={rowClassName}>
                                    <div className="flex items-center justify-between gap-3">
                                        <div className="flex min-w-0 flex-wrap items-center gap-2">
                                            <span className="font-medium">{label(r.trigger)}</span>
                                            <Badge variant={r.status === "failed" ? "destructive" : "outline"} className="font-normal">{label(r.status)}</Badge>
                                            <span className="text-xs text-muted-foreground">{new Date(r.time * 1000).toLocaleString()}</span>
                                        </div>
                                        {r.conversation && <Button size="sm" variant="ghost" className="shrink-0"
                                            onClick={() => showConversation(r.conversation!)}>查看过程</Button>}
                                    </div>
                                    {r.summary && <p className="whitespace-pre-wrap text-muted-foreground">{r.summary}</p>}
                                </article>)}</div>
                                : <p className="rounded-lg border px-4 py-6 text-center text-sm text-muted-foreground">暂无整理记录。</p>}
                            {content.deliveries.length > 0 && <section className="space-y-2">
                                <h3 className="text-sm font-medium">近期投递</h3>
                                <div className={listClassName}>{content.deliveries.map((d, index) => <article key={index} className={rowClassName}>
                                    <div className="flex flex-wrap items-center gap-2">
                                        <span className="font-medium">{d.title}</span>
                                        <Badge variant="outline" className="font-normal">{label(d.status)}</Badge>
                                        <span className="text-xs text-muted-foreground">{d.date}</span>
                                    </div>
                                    {d.fallback && <p className="text-muted-foreground">总管家代送：{d.fallback}</p>}
                                    {d.external && <p className={d.error ? "text-destructive" : "text-muted-foreground"}>
                                        飞书：{label(d.external)}{d.error ? `（${d.error}）` : ""}
                                    </p>}
                                </article>)}</div>
                            </section>}
                        </TabsContent>
                    </>}
                </div>
            </Tabs>
        </DialogContent>
    </Dialog>;
}
