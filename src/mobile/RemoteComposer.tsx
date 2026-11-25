import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import InputArea from "@/components/conversation/InputArea";
import { Button } from "@/components/ui/button";

export interface RemoteTarget {
    id: number;
    assistantId: number | null;
}
interface Pending { request: string; text: string }
interface Receipt { state: "pending" | "accepted" | "failed"; conversation: number | null; message: number | null; error: string | null }
export function chatAction<T>(id: string, action: object): Promise<T> {
    return invoke<T>("bridge_connect", { id, action });
}

export default function RemoteComposer({ hostId, target, running, onAccepted }: {
    hostId: string; target: RemoteTarget; running: boolean; onAccepted: (id: number) => void;
}) {
    const key = `aipp.mobile.host.${hostId}.chat.${target.id}`;
    const [draft, setDraft] = useState(() => localStorage.getItem(key + ".draft") ?? "");
    const [pending, setPending] = useState<Pending | null>(() => {
        try { return JSON.parse(localStorage.getItem(key + ".pending") ?? "null"); } catch { return null; }
    });
    const [error, setError] = useState("");
    const [sending, setSending] = useState(false);
    const [last, setLast] = useState(() => localStorage.getItem(key + ".last"));
    const submitting = useRef(false);
    const live = useRef(true);
    const accepted = useRef(onAccepted);
    accepted.current = onAccepted;
    useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
    useEffect(() => {
        if (draft) localStorage.setItem(key + ".draft",draft); else localStorage.removeItem(key + ".draft");
    }, [draft,key]);

    // A reconnect only queries the receipt. Only an explicit send transmits a message.
    useEffect(() => {
        if (!pending) return;
        let disposed = false;
        let timer: ReturnType<typeof setTimeout>;
        async function check() {
            try {
                const receipt = await chatAction<Receipt>(hostId,{ action:"receipt", request:pending!.request });
                if (disposed) return;
                if (receipt.state !== "pending") {
                    localStorage.removeItem(key + ".pending");
                    setPending(null);
                    if (receipt.state === "failed") setError(receipt.error ?? "电脑未确认发送结果");
                    else setError("");
                    if (receipt.conversation && receipt.message) {
                        localStorage.setItem(`aipp.mobile.host.${hostId}.chat.${receipt.conversation}.last`,pending!.request);
                        setLast(pending!.request);
                        if (localStorage.getItem(key + ".draft") === pending!.text) {
                            localStorage.removeItem(key + ".draft");
                            setDraft("");
                        } else if (target.id < 0) {
                            const edited = localStorage.getItem(key + ".draft");
                            if (edited) {
                                localStorage.setItem(`aipp.mobile.host.${hostId}.chat.${receipt.conversation}.draft`,edited);
                                localStorage.removeItem(key + ".draft");
                            }
                        }
                        if (receipt.state === "accepted") accepted.current(receipt.conversation);
                    }
                    return;
                }
            } catch (e) { if (!disposed) setError(String(e)); }
            if (!disposed) timer = setTimeout(() => void check(),2000);
        }
        timer = setTimeout(() => void check(),350);
        return () => { disposed = true; clearTimeout(timer); };
    }, [pending,key,hostId,target.id]);

    async function send(emptyPrompt?: string) {
        if (submitting.current) return;
        const text = draft.trim() ? draft : (emptyPrompt ?? "");
        if (running && !draft.trim() && !pending) {
            if (!last) { setError("只能停止由这部手机发起的回复"); return; }
            submitting.current = true; setSending(true);
            try {
                await chatAction(hostId,{ action:"stop",request:last });
                if (live.current) setError("");
            } catch (e) { if (live.current) setError(String(e)); }
            finally { submitting.current=false; if (live.current) setSending(false); }
            return;
        }
        if (running && !pending) { setError("电脑正在回复，草稿已保留"); return; }
        if (!pending && !text) return;
        if (pending && text !== pending.text && !emptyPrompt) {
            setError("上一条消息仍在等待电脑确认，当前草稿已保留"); return;
        }
        const item = pending ?? { request: crypto.randomUUID(), text };
        const destination = target.id < 0 ? {kind:"new",assistant:target.assistantId} : {kind:"existing",conversation:target.id};
        submitting.current=true; setSending(true); setError("");
        try {
            localStorage.setItem(key + ".pending",JSON.stringify(item));
            setPending(item);
            await chatAction(hostId,{ action:"send",request:item.request,destination,text:item.text });
        } catch (e) {
            if (live.current) setError(String(e));
            // A rejected request wasn't admitted. A transport error is ambiguous:
            // keep its ID so the retry cannot cause a second desktop execution.
            if (/HTTP 4\d\d/.test(String(e)) && localStorage.getItem(key + ".pending") === JSON.stringify(item)) {
                localStorage.removeItem(key + ".pending");
                if (live.current) setPending(null);
            }
        } finally { submitting.current=false; if (live.current) setSending(false); }
    }
    return <div className="shrink-0">
        {error && <p role="alert" className="px-3 text-sm text-destructive">{error}</p>}
        {pending && <div className="flex items-center gap-2 px-3 text-sm" role="status">
            等待电脑确认<Button size="sm" variant="ghost" disabled={sending} onClick={() => void send(pending.text)}>重试原消息</Button>
        </div>}
        <InputArea plainTextOnly isMobile inputText={draft} setInputText={setDraft} mentionCandidates={[]}
            handleSend={value => void send(value)} aiIsResponsing={running} placeholder="输入消息…" />
    </div>;
}
