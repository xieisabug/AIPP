import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle, DialogTrigger } from "@/components/ui/dialog";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import ButlerAvatar from "./ButlerAvatar";
import type { PeerPost } from "./butlerTimeline";

interface Episode {
    prompt: string | null;
    messages: { sender: string; recipient: string; text: string }[];
    status: string;
}

export default function ButlerExchangeRow({ posts, conversationId }: { posts: PeerPost[]; conversationId: number }) {
    const [open, setOpen] = useState(false);
    const [retry, setRetry] = useState(0);
    const [snapshot, setSnapshot] = useState<{ key: string; episodes: Episode[]; unavailable?: boolean }>();
    const ids = posts.map(p => p.id).join(",");
    const key = `${conversationId}:${ids}`;
    useEffect(() => {
        if (!open) return;
        let active = true;
        let revision = 0;
        const refresh = async () => {
            const request = ++revision;
            try {
                const episodes = await invoke<Episode[]>("get_butler_exchange_context", {
                    conversationId, postIds: ids.split(",").map(Number),
                });
                if (active && request === revision) setSnapshot({key, episodes});
            } catch (error) {
                console.error("[ButlerExchangeRow] context", {conversationId, ids, error});
                if (active && request === revision) setSnapshot({key, episodes: [], unavailable: true});
            }
        };
        const listeners = ["butler_peer_changed", "butler_task_updated", "butler_task_completed"].map(event => listen(event, refresh));
        void Promise.all(listeners).then(refresh).catch(error => {
            console.warn("[ButlerExchangeRow] subscription", error);
            void refresh();
        });
        return () => {
            active = false;
            listeners.forEach(listener => void listener.then(stop => stop()).catch(console.warn));
        };
    }, [open, conversationId, ids, key, retry]);
    const current = snapshot?.key === key ? snapshot : undefined;
    const first = posts[0];
    const people = [...new Map(posts.map(post => [post.peer_conversation, post])).values()];
    const sent = posts.every(p => p.outgoing);
    const incoming = posts.every(p => !p.outgoing);
    const incomplete = posts.some(p => ["failed", "stopped"].includes(p.status));
    const incomingRequest = incoming && posts.every(p => !p.task_reply);
    const label = incomplete ? "与" : sent ? (first.task_reply ? "已回复" : posts.some(p => ["queued", "sending"].includes(p.status)) ? "正在联系" : "已发消息给")
        : incomingRequest ? (posts.every(p => p.to_user) ? "接手工作，来自" : posts.every(p => p.to_user === false) ? "协助请求来自" : "消息来自")
        : incoming ? "消息来自" : `${posts.length} 条消息往来`;
    const directReport = posts.length === 1 && first.direct && !first.error && first.text.trim() !== "";
    const speaker = first.speaker ?? first.peer;
    const speakerAvatar = first.speaker_assistant ?? first.peer_assistant ?? first.peer_conversation;
    return <Dialog open={open} onOpenChange={setOpen}>
        {directReport ? <div className="butler-exchange" data-aipp-slot="butler-direct-report">
            <div className="flex items-center gap-2 text-xs text-muted-foreground">
                <ButlerAvatar id={speakerAvatar} size={20}/>
                <span className="font-medium text-foreground">{speaker}</span>
                <span>{first.task_id ? "交付任务结果" : "直接答复"}</span>
                <time dateTime={first.occurred_at}>
                    {new Date(first.occurred_at).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}
                </time>
                <DialogTrigger asChild>
                    <button type="button" className="ml-auto rounded px-1.5 py-0.5 hover:bg-muted hover:text-foreground">查看往来</button>
                </DialogTrigger>
            </div>
            <div className="butler-peer-bubble mt-1.5 max-w-full">
                <UnifiedMarkdown>{first.text}</UnifiedMarkdown>
            </div>
        </div> : <div className="butler-exchange" data-aipp-slot="butler-peer-event">
            <time className="block text-center text-[11px] text-muted-foreground" dateTime={first.occurred_at}>
                {new Date(first.occurred_at).toLocaleString(undefined, { month: "short", day: "numeric", hour: "2-digit", minute: "2-digit" })}
            </time>
            <div className="mt-2 flex justify-center">
                <DialogTrigger asChild>
                    <button type="button" className="inline-flex items-center gap-1.5 rounded-full px-2 py-1 text-xs text-muted-foreground hover:bg-muted hover:text-foreground">
                        {label}
                        <span className="flex -space-x-1">{people.slice(0, 4).map(p => <ButlerAvatar key={p.peer_conversation} id={p.peer_assistant ?? p.peer_conversation} size={17}/>)}</span>
                        {people.length === 1 ? first.peer : `${people.length} 个 Bot`}{incomplete ? "的对话" : ""}
                    </button>
                </DialogTrigger>
            </div>
        </div>}
        <DialogContent className="sm:max-w-xl max-h-[75vh] flex flex-col">
            <DialogHeader>
                <DialogTitle>助手对话</DialogTitle>
                <DialogDescription>这次委托的起因与助手往来</DialogDescription>
            </DialogHeader>
            <div className="min-h-0 overflow-y-auto space-y-5 text-sm" data-aipp-slot="butler-exchange-context">
                {!current && <p className="text-muted-foreground">正在读取…</p>}
                {current?.unavailable && <button onClick={() => setRetry(n => n + 1)} className="text-muted-foreground hover:underline">重新读取对话</button>}
                {current?.episodes.map((episode, index) => <section key={index} className="space-y-3">
                    {episode.prompt && <div className="rounded-lg border p-3 whitespace-pre-wrap break-words"><p className="mb-1 text-xs text-muted-foreground">你的委托</p>{episode.prompt}</div>}
                    {episode.messages.map((message, i) => <div key={i} className="rounded-lg bg-muted/60 p-3">
                        <p className="mb-1 text-xs text-muted-foreground">{message.sender} → {message.recipient}</p>
                        <div className="whitespace-pre-wrap break-words">{message.text}</div>
                    </div>)}
                    {!["delivered", "succeeded"].includes(episode.status) && <p className="text-xs text-muted-foreground">
                        {["failed", "stopped"].includes(episode.status) ? "本次对话未完成" : "等待后续回复…"}
                    </p>}
                </section>)}
            </div>
        </DialogContent>
    </Dialog>;
}
