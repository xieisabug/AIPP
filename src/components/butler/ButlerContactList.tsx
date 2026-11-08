import AssistantItemContextMenu, { type AssistantItemAction } from "@/components/config/assistant/AssistantItemContextMenu";
import { Search } from "lucide-react";
import type { AssistantListItem } from "@/data/Assistant";
import { useContactAvatarActivity } from "@/components/avatar/useContactAvatarActivity";
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
    mainConversationId: string;
    plugins: any[];
    error: string;
    onRefresh: () => void;
    onSelect: (assistant?: AssistantListItem) => void;
    /** 打开管家消息搜索（与 ChatUI 搜索一致，范围限定为管家模式产生的消息） */
    onSearch: () => void;
    onAssistantAction: (assistant: AssistantListItem, action: AssistantItemAction) => void;
}
export default function ButlerContactList({chief, contacts, selected, ready, previews, mainConversationId, plugins, error, onRefresh, onSelect, onSearch, onAssistantAction}: Props) {
    const { visuals, acknowledge } = useContactAvatarActivity(mainConversationId, previews, selected);
    // 只有真实活动（任务、回复、审批、完成/失败反馈）才驱动动画；空闲时头像保持静止。
    const state = (id: number) => visuals.get(id)?.state ?? "idle";
    const {assistantTypes} = useAssistantTypePlugin(plugins);
    return <>
        <div className="flex h-12 shrink-0 items-center justify-end gap-1.5 px-3">
            <Button size="icon" variant="outline" className="size-8 rounded-full" aria-label="搜索管家消息" title="搜索管家消息"
                disabled={!ready} onClick={onSearch} data-aipp-slot="butler-search"><Search/></Button>
            <AddAssistantDialog assistantTypes={assistantTypes} onAssistantAdded={onRefresh}
                triggerButtonProps={{variant:"outline",size:"icon",className:"size-8 rounded-full",title:"添加助手","aria-label":"添加助手"}}/>
        </div>
        <div className="mb-4 flex shrink-0 justify-center">
            <button onClick={() => { acknowledge?.(0); onSelect(); }} aria-pressed={selected === undefined}
                className="flex max-w-full flex-col items-center gap-1.5 rounded-xl px-5 py-2 text-xs font-medium hover:bg-muted aria-pressed:bg-muted">
                <ButlerAvatar id={0} chief size={44} figure animated={state(0) !== "idle"} state={state(0)}/>{chief}
                {visuals.get(0)?.label && <span className="max-w-40 truncate text-xs text-muted-foreground">{visuals.get(0)?.label}</span>}
            </button>
        </div>
        <nav aria-label="助手联系人" className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
            {error && <button onClick={onRefresh} className="px-2 py-1 text-xs text-destructive">刷新联系人</button>}
            {contacts.map(assistant => {
                const preview = previews.find(p => p.assistantId === assistant.id);
                const unread = preview?.messageId && selected !== assistant.id &&
                    preview.messageId > Number(localStorage.getItem(`butler-read-${preview.conversationId}`));
                return <AssistantItemContextMenu key={assistant.id} assistant={assistant} onAction={onAssistantAction} canClear><button onClick={() => { acknowledge?.(assistant.id); onSelect(assistant); }} disabled={!ready}
                    aria-pressed={selected === assistant.id} className="butler-contact">
                    <ButlerAvatar id={assistant.id} figure animated={state(assistant.id) !== "idle"} state={state(assistant.id)} />
                    <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium">{assistant.name}</span>
                        <span className="block truncate text-xs text-muted-foreground">{visuals.get(assistant.id)?.label || preview?.preview || "尚未对话"}</span>
                    </span>
                    {unread ? <span aria-label="未读" className="size-2 shrink-0 rounded-full bg-primary"/> : null}
                </button></AssistantItemContextMenu>;
            })}
        </nav>
    </>;
}
