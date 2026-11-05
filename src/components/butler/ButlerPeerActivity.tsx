import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";

interface Exchange {
    id: number; root: number; owner: number; target: number;
    sender: string; recipient: string; body: string; reply: string | null;
    error: string | null; round_no: number; max_rounds: number; state: string; stop_requested: boolean;
}
const labels: Record<string,string> = {
    queued: "排队中", sending: "投递中", waiting: "等待答复", returning: "答复待回送",
    reply_sending: "正在回送", delivered: "已回送", failed: "失败", stopped: "已停止",
};
export default function ButlerPeerActivity({ conversationId }: {conversationId: number}) {
    const [rows, setRows] = useState<Exchange[]>([]);
    const [open, setOpen] = useState(false);
    const [error, setError] = useState("");
    useEffect(() => {
        let active = true;
        let revision = 0;
        const load = async () => {
            const request = ++revision;
            try {
                const result = await invoke<Exchange[]>("list_butler_peer_exchanges", {conversationId,includeDetails:open});
                if (active && request === revision) { setRows(result); setError(""); }
            } catch (e) { if (active && request === revision) setError(String(e)); }
        };
        const listener = listen("butler_peer_changed", load);
        void listener.then(load).catch(e => { if (active) setError(String(e)); });
        return () => { active = false; void listener.then(stop => stop()).catch(console.warn); };
    }, [conversationId,open]);
    const latest = rows.find(row => !["failed", "stopped", "delivered"].includes(row.state)) ?? rows[0];
    if (!latest && !error) return null;
    const status = error ? "读取协作失败" : `协作 · ${latest.owner === conversationId ? latest.recipient : latest.sender} · ${labels[latest.state]}`;
    return <Dialog open={open} onOpenChange={setOpen}>
        <Button onClick={() => setOpen(true)} variant="ghost" size="sm" aria-label={status} title={status}>
            {error ? "读取协作失败" : "协作记录"}
        </Button>
        <DialogContent className="max-h-[80vh] overflow-y-auto">
            <DialogHeader>
                <DialogTitle>助手协作记录</DialogTitle>
                <DialogDescription>按轮查看请求和答复。停止后续协作后，已经在运行的助手可以在它自己的聊天中停止。</DialogDescription>
            </DialogHeader>
            {error && <p role="alert">{error}</p>}
            {rows.map(ex => <article key={ex.id} className="space-y-2 border-t py-3 text-sm">
                <p className="font-medium">{ex.sender} → {ex.recipient} · 第 {ex.round_no}/{ex.max_rounds} 轮 · {labels[ex.state]}{ex.stop_requested && ex.state !== "stopped" ? "（后续已停止）" : ""}</p>
                <p className="whitespace-pre-wrap break-words">{ex.body}</p>
                {ex.reply && <div className="whitespace-pre-wrap break-words rounded bg-muted p-2">{ex.reply}</div>}
                {ex.error && <p role="alert" className="break-words text-destructive">{ex.error}</p>}
                {!ex.stop_requested && !["failed","stopped"].includes(ex.state) && rows.find(row => row.root === ex.root)?.id === ex.id &&
                    <Button size="sm" variant="outline" onClick={async () => {
                        try { await invoke("stop_butler_peer_exchange", {conversationId,root:ex.root}); }
                        catch (e) { toast.error(String(e)); }
                    }}>停止后续协作</Button>}
            </article>)}
        </DialogContent>
    </Dialog>;
}
