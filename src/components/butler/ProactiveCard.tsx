import { useEffect, useState } from "react";
import { toast } from "sonner";
import { invoke } from "@tauri-apps/api/core";
import { Button } from "@/components/ui/button";
import MessagePreviewToolCall from "@/components/mcp-tool-components/MessagePreviewToolCall";
import { accept, changeItem, showConversation, type Confirmation, type Delivery } from "./proactiveApi";

export default function ProactiveCard({ data, conversation, message, grouped }: { data: Delivery; conversation: number; message: number; grouped?: boolean }) {
    const [local, setLocal] = useState(false);
    const [confirmation, setConfirmation] = useState<Confirmation>();
    const [busy, setBusy] = useState(false);
    const [receipt, setReceipt] = useState("");
    const [showBrief, setShowBrief] = useState(false);
    const [draft, setDraft] = useState<string>();
    const [item, setItem] = useState<{revision:number;title:string}>();
    useEffect(() => {
        let live = true;
        void invoke<{ item: number; delivery?: number; state: string; status: string; revision:number; title:string } | null>("get_butler_proactive_card",
            { uuid: data.uuid, conversation, message }).then(async row => {
            if (!live) return;
            if (!row || row.item !== data.item || row.delivery !== data.delivery) { setReceipt("此提醒可在发出它的设备上管理"); return; }
            setLocal(true);
            setItem({revision:row.revision,title:row.title});
            if (row.state === "dismissed" || row.status === "cancelled") { setReceipt("已停止跟进"); return; }
            if (row.status === "acknowledged" || (row.state === "done" && data.kind !== "proposal")) { setReceipt("已确认本次提醒"); return; }
            if (row.status === "queued") { setReceipt("已推迟提醒"); return; }
            if (data.kind === "proposal") {
                const reply = await accept(data.item, data.revision);
                if (live) setConfirmation(reply);
            }
        }).catch(error => { if (live) setReceipt(String(error)); });
        return () => { live = false; };
    }, [data.item, data.kind, data.revision, data.uuid, data.delivery, conversation, message]);
    useEffect(() => {
        if (confirmation?.status !== "pending") return;
        let live=true;
        const poll=window.setInterval(() => {
            void accept(data.item,data.revision).then(result => {
                if (live) setConfirmation(result);
            }).catch(error => { if(live) setReceipt(String(error)); });
        },3000);
        return () => {live=false;window.clearInterval(poll);};
    }, [confirmation?.status,data.item,data.revision]);
    async function act(action: string) {
        setBusy(true);
        try {
            if (action === "edit" && draft !== undefined && confirmation?.revision) {
                await changeItem(data.item, confirmation.revision, "edit_proposal", undefined, draft);
                setConfirmation(await accept(data.item, confirmation.revision));
                setDraft(undefined);
            } else if (action === "accept" && confirmation?.assistant && confirmation.revision) {
                const reply = await accept(data.item, confirmation.revision, confirmation.assistant, confirmation.instruction);
                setConfirmation(reply);
                if (reply.status === "sent" && reply.conversation) showConversation(reply.conversation, reply.message);
            } else {
                await changeItem(data.item, confirmation?.revision ?? item?.revision ?? data.revision, action, data.delivery);
                setReceipt(action === "snooze" ? "明天再提醒" : action === "dismiss" ? "已停止跟进" : "已确认本次提醒");
            }
        } catch (error) { toast.error(String(error)); }
        finally { setBusy(false); }
    }
    return <div className="mt-2 space-y-2 border-t pt-2 text-xs">
        {grouped && item?.title && <p>{item.title}</p>}
        <div className="flex items-center gap-3 text-muted-foreground">
            <span>{data.kind === "proposal" ? "主动建议" : data.kind === "milestone" ? "纪念日关怀" : data.brief ? "提前准备" : "管家整理"}</span>
            {local && <button type="button" className="underline" onClick={() => showConversation(data.source, data.position)}>查看原话</button>}
            {local && data.brief && <button type="button" className="underline" onClick={() => setShowBrief(!showBrief)}>{showBrief ? "收起简报" : "查看简报"}</button>}
        </div>
        {showBrief && data.brief && <MessagePreviewToolCall conversationId={conversation} parameters={JSON.stringify({ message_id: data.brief, title: "预研简报" })} />}
        {confirmation?.status === "confirm" && <p className="whitespace-pre-wrap">{confirmation.redirected ? "原联系人不可用，改由" : "交给"} {confirmation.name} 执行：{confirmation.instruction}</p>}
        {draft !== undefined && <label className="block">待执行指令
            <textarea className="w-full rounded border bg-background p-2" value={draft} onChange={e => setDraft(e.target.value)} />
            <Button size="sm" disabled={busy || !draft.trim()} onClick={() => void act("edit")}>保存并核对</Button>
            <Button size="sm" variant="ghost" onClick={() => setDraft(undefined)}>取消</Button></label>}
        {receipt || confirmation?.status === "sent" || confirmation?.status === "pending"
            ? <p role="status">{receipt || (confirmation?.status === "sent" ? "已派发" : "等待目标会话空闲")}</p>
            : local && <div className="flex flex-wrap gap-1">
                {data.kind === "proposal" ? <><Button size="sm" disabled={busy || !confirmation || draft !== undefined} onClick={() => void act("accept")}>采纳并执行</Button>
                    <Button size="sm" variant="ghost" disabled={busy || !confirmation} onClick={() => setDraft(confirmation?.instruction)}>改一下再发</Button></>
                    : <><Button size="sm" variant="outline" disabled={busy} onClick={() => void act("done")}>知道了</Button>
                    <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act("snooze")}>明天再提醒</Button></>}
                <Button size="sm" variant="ghost" disabled={busy} onClick={() => void act("dismiss")}>{data.kind === "proposal" ? "暂不需要" : "不再跟进"}</Button>
            </div>}
    </div>;
}
