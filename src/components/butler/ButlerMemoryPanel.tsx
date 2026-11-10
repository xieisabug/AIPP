import { type ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { Brain, Loader2, Pencil, Quote, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { BUTLER_MEMORY_MAX_CHARS, type ButlerMemory, type ButlerMemorySource } from "@/data/ButlerMemory";
import * as memoryApi from "@/services/butlerMemoryService";

interface Props {
    /** 私聊/总管家会话或群聊的会话 id，决定能看到哪些记忆。 */
    conversationId: number;
    /** assistant：私聊与总管家（该助手 + 全局）；group：群聊（本群 + 全局）。 */
    scope: "assistant" | "group";
}

type SourceState = ButlerMemorySource | "loading" | { error: string };

const COPY = {
    assistant: {
        intro: "在聊天里说“记住……”，助手会把你的偏好、事实和决定记在这里。每轮对话会带上与当前内容最相关的几条，修改和删除立即生效。",
        ownTitle: "该助手的记忆", ownHint: "只有这位助手能看到", forgetter: "助手",
        emptyHint: "在聊天里对助手说“记住……”，它记下的偏好和决定会出现在这里。",
    },
    group: {
        intro: "在群里说“大家记住……”，成员会把约定记在这里，本群成员共享。修改和删除立即生效。",
        ownTitle: "本群记忆", ownHint: "本群成员共享", forgetter: "群成员",
        emptyHint: "在群里说“大家记住……”，成员记下的约定会出现在这里。",
    },
};

const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

function formatTime(ms: number) {
    const date = new Date(ms);
    const pad = (n: number) => String(n).padStart(2, "0");
    const day = `${date.getMonth() + 1}月${date.getDate()}日 ${pad(date.getHours())}:${pad(date.getMinutes())}`;
    return date.getFullYear() === new Date().getFullYear() ? day : `${date.getFullYear()}年${day}`;
}

function ErrorBox({ children }: { children: ReactNode }) {
    return <div role="alert" className="whitespace-pre-wrap break-words rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
        {children}
    </div>;
}

function SourceBlock({ state }: { state: SourceState }) {
    let body: ReactNode;
    if (state === "loading") body = <span className="inline-flex items-center gap-1.5 text-muted-foreground"><Loader2 className="size-3.5 animate-spin" />正在读取来源…</span>;
    else if ("error" in state) body = <span role="alert" className="text-destructive">{state.error}</span>;
    else if (state.status === "none") body = <span className="text-muted-foreground">没有记录来源</span>;
    else if (state.status === "deleted") body = <span className="text-muted-foreground">原消息已删除，记忆仍然保留</span>;
    else body = <>
        <span className="block text-xs text-muted-foreground">来源：{state.label}</span>
        <span className="mt-1 block whitespace-pre-wrap break-words leading-6 text-foreground/80">{state.text}{state.truncated ? "…" : ""}</span>
    </>;
    return <div className="mt-3 border-l-2 border-border pl-3 text-sm" data-aipp-slot="butler-memory-source">{body}</div>;
}

/** 长期记忆列表：查看来源、修改与删除，改动立即生效。 */
export default function ButlerMemoryPanel({ conversationId, scope }: Props) {
    const copy = COPY[scope];
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
            if (current === request.current) setLoadError(`读取记忆失败（会话 ${conversationId}）：${errorText(error)}`);
        }
    }, [conversationId]);

    useEffect(() => {
        void load();
        let disposed = false;
        let unlisten: (() => void) | undefined;
        memoryApi.onButlerMemoryChanged(() => void load())
            .then(stop => { if (disposed) stop(); else unlisten = stop; })
            .catch(error => console.warn("监听记忆变化失败", error));
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

    const iconButton = "size-8 text-muted-foreground hover:text-foreground";

    const item = (memory: ButlerMemory) => {
        const draft = editing && editing.id === memory.id ? editing.draft : null;
        const busy = busyId === memory.id;
        return <li key={memory.id} data-aipp-slot="butler-memory-item"
            className="group/memory rounded-lg border border-border bg-background px-4 py-3 transition-colors hover:border-foreground/20">
            {draft !== null ? <div className="space-y-2">
                <Textarea aria-label={`编辑记忆 #${memory.id}`} value={draft} rows={3} maxLength={BUTLER_MEMORY_MAX_CHARS} autoFocus
                    className="min-h-20 text-sm leading-6" onChange={event => setEditing({ id: memory.id, draft: event.target.value })} />
                <div className="flex items-center justify-between gap-2">
                    <span className="text-xs tabular-nums text-muted-foreground">{draft.trim().length} / {BUTLER_MEMORY_MAX_CHARS}</span>
                    <div className="flex gap-2">
                        <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => setEditing(null)}>取消</Button>
                        <Button type="button" size="sm" disabled={busy || !draft.trim()} onClick={() => void save(memory, draft)}>
                            {busy && <Loader2 className="size-3.5 animate-spin" />}保存
                        </Button>
                    </div>
                </div>
            </div> : <div className="flex items-start gap-3">
                <p className="min-w-0 flex-1 whitespace-pre-wrap break-words text-sm leading-6">{memory.content}</p>
                {/* 操作在悬停或键盘聚焦时出现，平时只看内容。 */}
                <div className="-mr-2 -mt-1 flex shrink-0 items-center opacity-0 transition-opacity group-hover/memory:opacity-100 group-focus-within/memory:opacity-100">
                    <Button type="button" variant="ghost" size="icon" className={iconButton} title="查看来源" aria-label="查看来源"
                        aria-expanded={Boolean(sources[memory.id])} onClick={() => void toggleSource(memory.id)}><Quote className="size-4" /></Button>
                    <Button type="button" variant="ghost" size="icon" className={iconButton} title="修改" aria-label={`修改记忆 #${memory.id}`}
                        onClick={() => { setEditing({ id: memory.id, draft: memory.content }); setConfirmingId(null); }}><Pencil className="size-4" /></Button>
                    <Button type="button" variant="ghost" size="icon" className={iconButton} title="删除" aria-label={`删除记忆 #${memory.id}`}
                        onClick={() => { setConfirmingId(memory.id); setEditing(null); }}><Trash2 className="size-4" /></Button>
                </div>
            </div>}
            <p className="mt-1.5 text-xs text-muted-foreground">更新于 {formatTime(memory.updated_at)}</p>
            {sources[memory.id] && <SourceBlock state={sources[memory.id]} />}
            {confirmingId === memory.id && <div className="mt-3 flex flex-wrap items-center justify-between gap-2 rounded-md bg-muted px-3 py-2">
                <span className="text-sm">删除后{copy.forgetter}不再记得这条。</span>
                <div className="flex gap-2">
                    <Button type="button" size="sm" variant="ghost" disabled={busy} onClick={() => setConfirmingId(null)}>取消</Button>
                    <Button type="button" size="sm" variant="destructive" disabled={busy} onClick={() => void remove(memory)}>
                        {busy && <Loader2 className="size-3.5 animate-spin" />}确认删除
                    </Button>
                </div>
            </div>}
        </li>;
    };

    const section = (title: string, hint: string, list: ButlerMemory[], empty: string) => <section aria-label={title} className="space-y-2.5">
        <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-0.5">
            <h3 className="text-sm font-medium">{title}<span className="ml-2 text-xs font-normal tabular-nums text-muted-foreground">{list.length}</span></h3>
            <span className="text-xs text-muted-foreground">{hint}</span>
        </div>
        {list.length ? <ul className="space-y-2">{list.map(item)}</ul>
            : <p className="rounded-lg border border-dashed border-border px-4 py-3 text-sm text-muted-foreground">{empty}</p>}
    </section>;

    let content: ReactNode;
    if (memories === null) {
        content = !loadError && <div className="flex flex-1 items-center justify-center gap-2 py-12 text-sm text-muted-foreground">
            <Loader2 className="size-4 animate-spin" />正在读取记忆…
        </div>;
    } else if (memories.length === 0) {
        content = <div className="flex flex-1 flex-col items-center justify-center gap-3 rounded-xl border border-dashed border-border px-6 py-12 text-center">
            <div className="flex size-12 items-center justify-center rounded-full bg-muted"><Brain className="size-6 text-muted-foreground" /></div>
            <div className="space-y-1">
                <p className="text-sm font-medium">还没有记忆</p>
                <p className="max-w-xs text-sm leading-6 text-muted-foreground">{copy.emptyHint}</p>
            </div>
        </div>;
    } else {
        content = <>
            {section(copy.ownTitle, copy.ownHint, memories.filter(memory => memory.scope_type !== "global"), "还没有记忆。")}
            {section("全局记忆", "所有助手都能看到，包括群聊", memories.filter(memory => memory.scope_type === "global"), "还没有全局记忆。")}
        </>;
    }

    return <div className="flex min-h-full flex-col gap-5" data-aipp-slot="butler-memory-panel">
        <p className="text-sm leading-6 text-muted-foreground">{copy.intro}</p>
        {loadError && <ErrorBox>{loadError}</ErrorBox>}
        {actionError && <ErrorBox>{actionError}</ErrorBox>}
        {content}
    </div>;
}
