import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { Loader2, Pencil, Quote, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { BUTLER_MEMORY_MAX_CHARS, type ButlerMemory, type ButlerMemorySource } from "@/data/ButlerMemory";
import * as memoryApi from "@/services/butlerMemoryService";

interface Props {
    /** 私聊/总管家会话或群聊的会话 id，决定能看到哪些记忆。 */
    conversationId: number;
    /** 本范围的说法，例如“该助手”“本群”。 */
    ownScopeLabel: string;
}

type SourceState = ButlerMemorySource | "loading" | { error: string };

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

function formatTime(ms: number) {
    const date = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}

function SourceBlock({ state }: { state: SourceState }) {
    let body: ReactNode;
    if (state === "loading") body = <span className="text-muted-foreground">正在读取来源…</span>;
    else if ("error" in state) body = <span role="alert" className="text-destructive">{state.error}</span>;
    else if (state.status === "none") body = <span className="text-muted-foreground">没有记录来源</span>;
    else if (state.status === "deleted") body = <span className="text-muted-foreground">原消息已删除，记忆仍然保留</span>;
    else body = <>
        <span className="block text-muted-foreground">{state.label}</span>
        <span className="block whitespace-pre-wrap break-words">{state.text}{state.truncated ? "…" : ""}</span>
    </>;
    return <div className="mt-2 rounded-md bg-muted/60 px-2.5 py-2 text-xs" data-aipp-slot="butler-memory-source">{body}</div>;
}

/** 长期记忆列表：查看来源、修改与删除，改动立即生效。 */
export default function ButlerMemoryPanel({ conversationId, ownScopeLabel }: Props) {
    const [memories, setMemories] = useState<ButlerMemory[] | null>(null);
    const [loadError, setLoadError] = useState("");
    const [actionError, setActionError] = useState("");
    const [editing, setEditing] = useState<{ id: number; draft: string } | null>(null);
    const [confirmingId, setConfirmingId] = useState<number | null>(null);
    const [busyId, setBusyId] = useState<number | null>(null);
    const [sources, setSources] = useState<Record<number, SourceState>>({});
    const request = useRef(0);

    const load = useCallback(async () => {
        const current = ++request.current;
        try {
            const list = await memoryApi.listButlerMemories(conversationId);
            if (current !== request.current) return;
            setMemories(list);
            setLoadError("");
        } catch (error) {
            if (current === request.current) setLoadError(`读取长期记忆失败（会话 ${conversationId}）：${errorText(error)}`);
        }
    }, [conversationId]);

    useEffect(() => {
        void load();
        let disposed = false;
        let unlisten: (() => void) | undefined;
        memoryApi.onButlerMemoryChanged(() => void load())
            .then(stop => { if (disposed) stop(); else unlisten = stop; })
            .catch(error => console.warn("监听长期记忆变化失败", error));
        return () => { disposed = true; unlisten?.(); };
    }, [load]);

    const save = async (memory: ButlerMemory, draft: string) => {
        setBusyId(memory.id);
        setActionError("");
        try {
            await memoryApi.updateButlerMemory(memory.id, draft.trim(), memory.revision);
            setEditing(null);
            await load();
        } catch (error) {
            setActionError(`保存记忆 #${memory.id} 失败：${errorText(error)}`);
        } finally {
            setBusyId(null);
        }
    };

    const remove = async (memory: ButlerMemory) => {
        setBusyId(memory.id);
        setActionError("");
        try {
            await memoryApi.deleteButlerMemory(memory.id, memory.revision);
            setConfirmingId(null);
            await load();
        } catch (error) {
            setActionError(`删除记忆 #${memory.id} 失败：${errorText(error)}`);
        } finally {
            setBusyId(null);
        }
    };

    const toggleSource = async (id: number) => {
        if (sources[id]) {
            setSources(previous => {
                const next = { ...previous };
                delete next[id];
                return next;
            });
            return;
        }
        setSources(previous => ({ ...previous, [id]: "loading" }));
        try {
            const source = await memoryApi.getButlerMemorySource(id);
            setSources(previous => ({ ...previous, [id]: source }));
        } catch (error) {
            setSources(previous => ({ ...previous, [id]: { error: `读取来源失败：${errorText(error)}` } }));
        }
    };

    const item = (memory: ButlerMemory) => {
        const draft = editing && editing.id === memory.id ? editing.draft : null;
        const busy = busyId === memory.id;
        return <li key={memory.id} className="rounded-lg border border-border px-3 py-2" data-aipp-slot="butler-memory-item">
            {draft !== null ? <div className="space-y-2">
                <Textarea aria-label={`编辑记忆 #${memory.id}`} value={draft} rows={3} maxLength={BUTLER_MEMORY_MAX_CHARS}
                    onChange={event => setEditing({ id: memory.id, draft: event.target.value })} />
                <div className="flex items-center justify-between gap-2">
                    <span className="text-[11px] text-muted-foreground">{draft.trim().length}/{BUTLER_MEMORY_MAX_CHARS}</span>
                    <div className="flex gap-1">
                        <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => setEditing(null)}>取消</Button>
                        <Button type="button" size="sm" disabled={busy || !draft.trim()} onClick={() => void save(memory, draft)}>
                            {busy && <Loader2 className="size-3.5 animate-spin" />}保存
                        </Button>
                    </div>
                </div>
            </div> : <p className="whitespace-pre-wrap break-words text-sm">{memory.content}</p>}
            <div className="mt-1.5 flex items-center gap-1 text-[11px] text-muted-foreground">
                <span className="flex-1">更新于 {formatTime(memory.updated_at)}</span>
                <Button type="button" variant="ghost" size="sm" className="h-6 gap-1 px-1.5 text-[11px]" aria-expanded={Boolean(sources[memory.id])}
                    onClick={() => void toggleSource(memory.id)}><Quote className="size-3" />来源</Button>
                {draft === null && <Button type="button" variant="ghost" size="sm" className="h-6 px-1.5" title="修改" aria-label={`修改记忆 #${memory.id}`}
                    onClick={() => { setEditing({ id: memory.id, draft: memory.content }); setConfirmingId(null); }}><Pencil className="size-3" /></Button>}
                <Button type="button" variant="ghost" size="sm" className="h-6 px-1.5" title="删除" aria-label={`删除记忆 #${memory.id}`}
                    onClick={() => setConfirmingId(memory.id)}><Trash2 className="size-3" /></Button>
            </div>
            {confirmingId === memory.id && <div className="mt-1.5 flex items-center justify-end gap-1 text-xs">
                <span className="mr-auto text-muted-foreground">删除后助手不再记得这条。</span>
                <Button type="button" size="sm" variant="ghost" className="h-7" disabled={busy} onClick={() => setConfirmingId(null)}>取消</Button>
                <Button type="button" size="sm" variant="destructive" className="h-7" disabled={busy} onClick={() => void remove(memory)}>
                    {busy && <Loader2 className="size-3.5 animate-spin" />}确认删除
                </Button>
            </div>}
            {sources[memory.id] && <SourceBlock state={sources[memory.id]} />}
        </li>;
    };

    const section = (title: string, list: ButlerMemory[], empty: string) => <section className="space-y-1.5">
        <h4 className="text-xs text-muted-foreground">{title} · {list.length}</h4>
        {list.length ? <ul className="space-y-1.5">{list.map(item)}</ul> : <p className="text-xs text-muted-foreground">{empty}</p>}
    </section>;

    return <div className="space-y-3" data-aipp-slot="butler-memory-panel">
        <p className="text-xs text-muted-foreground">
            在聊天里说“记住……”，助手会保存偏好、事实和决定；每轮对话按与当前内容的相关程度附带一部分给模型。也可以直接在聊天里让助手更正或忘记。
        </p>
        {loadError && <p role="alert" className="whitespace-pre-wrap break-words text-xs text-destructive">{loadError}</p>}
        {actionError && <p role="alert" className="whitespace-pre-wrap break-words text-xs text-destructive">{actionError}</p>}
        {memories === null ? !loadError && <p className="text-xs text-muted-foreground">正在读取…</p> : <>
            {section(`仅${ownScopeLabel}`, memories.filter(memory => memory.scope_type !== "global"), "还没有记忆。")}
            {section("全局 · 所有助手可见", memories.filter(memory => memory.scope_type === "global"), "还没有全局记忆。")}
        </>}
    </div>;
}
