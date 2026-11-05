import { useState } from "react";
import { Search, X } from "lucide-react";
import type { AssistantListItem } from "@/data/Assistant";
import type { ButlerTaskListItem } from "@/data/Butler";
import { Button } from "@/components/ui/button";
import AddAssistantDialog from "@/components/config/AddAssistantDialog";
import { useAssistantTypePlugin } from "@/hooks/assistant/useAssistantTypePlugin";
import ButlerAvatar from "./ButlerAvatar";

export interface ContactPreview { assistantId: number; conversationId: number; messageId: number | null; preview: string | null }
interface Props {
    chief: string;
    contacts: AssistantListItem[];
    selected?: number;
    ready: boolean;
    previews: ContactPreview[];
    tasks: ButlerTaskListItem[];
    plugins: any[];
    error: string;
    onRefresh: () => void;
    onSelect: (assistant?: AssistantListItem) => void;
}
export default function ButlerContactList({chief, contacts, selected, ready, previews, tasks, plugins, error, onRefresh, onSelect}: Props) {
    const [searching, setSearching] = useState(false);
    const [query, setQuery] = useState("");
    const {assistantTypes} = useAssistantTypePlugin(plugins);
    const filtered = contacts.filter(a => a.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()));
    return <>
        <div className="flex h-12 shrink-0 items-center justify-end gap-1.5 px-3">
            {searching && <input autoFocus aria-label="搜索联系人" placeholder="搜索联系人" value={query}
                onChange={e => setQuery(e.target.value)} className="min-w-0 flex-1 rounded-md bg-background px-2 py-1 text-sm" />}
            <Button size="icon" variant="outline" className="size-8 rounded-full" aria-label={searching ? "关闭搜索" : "搜索联系人"}
                onClick={() => {setSearching(!searching); setQuery("");}}>{searching ? <X/> : <Search/>}</Button>
            <AddAssistantDialog assistantTypes={assistantTypes} onAssistantAdded={onRefresh}
                triggerButtonProps={{variant:"outline",size:"icon",className:"size-8 rounded-full",title:"添加助手","aria-label":"添加助手"}}/>
        </div>
        <div className="mb-4 flex shrink-0 justify-center">
            <button onClick={() => onSelect()} aria-pressed={selected === undefined}
                className="flex max-w-full flex-col items-center gap-1.5 rounded-xl px-5 py-2 text-xs font-medium hover:bg-muted aria-pressed:bg-muted">
                <ButlerAvatar id={0} chief size={44}/>{chief}
            </button>
        </div>
        <nav aria-label="助手联系人" className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
            {error && <button onClick={onRefresh} className="px-2 py-1 text-xs text-destructive">刷新联系人</button>}
            {filtered.map(assistant => {
                const preview = previews.find(p => p.assistantId === assistant.id);
                const unread = preview?.messageId && selected !== assistant.id &&
                    preview.messageId > Number(localStorage.getItem(`butler-read-${preview.conversationId}`));
                return <button key={assistant.id} onClick={() => onSelect(assistant)} disabled={!ready}
                    aria-pressed={selected === assistant.id} className="butler-contact">
                    <ButlerAvatar id={assistant.id} />
                    <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium">{assistant.name}</span>
                        <span className="block truncate text-xs text-muted-foreground">{preview?.preview || "尚未对话"}</span>
                    </span>
                    {tasks.some(t => t.executor_assistant_id === assistant.id && t.has_pending_permission)
                        ? <span className="text-xs text-muted-foreground">待审批</span>
                        : unread ? <span aria-label="未读" className="size-2 rounded-full bg-primary"/> : null}
                </button>;
            })}
            {query && !filtered.length && <p className="p-2 text-xs text-muted-foreground">没有匹配的助手</p>}
        </nav>
    </>;
}
