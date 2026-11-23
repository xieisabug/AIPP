import AssistantItemContextMenu, { type AssistantItemAction } from "@/components/config/assistant/AssistantItemContextMenu";
import { CalendarHeart, Search, Settings, UsersRound } from "lucide-react";
import type { AssistantListItem } from "@/data/Assistant";
import type { ButlerGroupSummary } from "@/data/ButlerGroup";
import type { useContactAvatarActivity } from "@/components/avatar/useContactAvatarActivity";
import { Button } from "@/components/ui/button";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import AddAssistantDialog from "@/components/config/AddAssistantDialog";
import { useAssistantTypePlugin } from "@/hooks/assistant/useAssistantTypePlugin";
import { usePendingApprovalConversations } from "@/hooks/useApprovalAttention";
import ButlerSortableList from "./ButlerSortableList";
import ButlerAvatar from "./ButlerAvatar";
import ButlerGroupAvatar from "./ButlerGroupAvatar";
import { groupPreview } from "./butlerGroupModel";
import { AppUpdateNotice } from "@/components/AppUpdateNotice";

/** 待审批和头像未读标记分别展示。 */
function ApprovalDot() {
    return <span aria-label="待审批" title="等待审批" className="size-2 shrink-0 rounded-full bg-amber-500"/>;
}

function UnreadDot() {
    return <span aria-label="未读" className="size-2 shrink-0 rounded-full bg-primary"/>;
}

export interface ContactPreview { assistantId: number; conversationId: number; messageId: number | null; replyId?: number | null; unread?: boolean; preview: string | null }
interface Props {
    focused?: boolean;
    chief: string;
    contacts: AssistantListItem[];
    selected?: number;
    ready: boolean;
    previews: ContactPreview[];
    plugins: any[];
    error: string;
    onRefresh: () => void;
    onSelect: (assistant?: AssistantListItem) => void;
    /** 打开管家消息搜索（与 ChatUI 搜索一致，范围限定为管家模式产生的消息） */
    onSearch: () => void;
    /** 打开总管家设置。入口在联系人列表左上角。 */
    onOpenSettings: () => void;
    /** 打开主动整理（跟进事项与整理手记）。只在主动整理已开启时传入，不传则不显示入口。 */
    onOpenProactive?: () => void;
    onAssistantAction: (assistant: AssistantListItem, action: AssistantItemAction) => void;
    /** 群聊与联系人在同一个列表里，选中群聊时 selected 联系人为空。 */
    groups: ButlerGroupSummary[];
    selectedGroup?: number;
    onSelectGroup: (group: ButlerGroupSummary) => void;
    onCreateGroup: () => void;
    onDeleteGroup: (group: ButlerGroupSummary) => void;
    /** 与顶部头像共用一份活动状态，避免两处各自订阅后动作错开。 */
    activity: ReturnType<typeof useContactAvatarActivity>;
}

function groupUnread(group: ButlerGroupSummary, selected: boolean) {
    return !selected && !!group.unread;
}

export default function ButlerContactList({chief, contacts, selected, focused, ready, previews, plugins, error, onRefresh, onSelect, onSearch, onOpenSettings, onOpenProactive, onAssistantAction,
    groups, selectedGroup, onSelectGroup, onCreateGroup, onDeleteGroup, activity}: Props) {
    const { visuals, approvals, acknowledge } = activity;
    // 群成员在各自的执行会话里请求审批，群聊行按成员会话汇总。
    const pendingApprovals = usePendingApprovalConversations();
    // 只有真实活动（任务、回复、审批、完成/失败反馈）才驱动动画；空闲时头像保持静止。
    const state = (id: number) => visuals.get(id)?.state ?? "idle";
    const {assistantTypes} = useAssistantTypePlugin(plugins);
    const groupRows = groups.map(group => {
        const active = selectedGroup === group.id;
        const unread = groupUnread(group, !!focused && active);
        const speaker = group.last_message?.sender_member;
        const typing = group.members.filter(member => group.typing.includes(member.id)).map(member => member.name);
        return { id: `group:${group.id}`, label: group.name, content: <ContextMenu>
            <ContextMenuTrigger asChild><div className="w-full">
                <button onClick={() => onSelectGroup(group)} disabled={!ready} aria-pressed={active} className="butler-contact" data-aipp-slot="butler-group-contact">
                    <ButlerGroupAvatar members={group.members} size={48} unreadMember={unread ? speaker : null} />
                    <span className="min-w-0 flex-1">
                        <span className="block truncate text-sm font-medium">{group.name}</span>
                        <span className="block truncate text-xs text-muted-foreground">
                            {typing.length ? `${typing.join("、")} 正在输入…` : groupPreview(group.last_message) || "尚无消息"}
                        </span>
                    </span>
                    {group.members.some(member => pendingApprovals.has(member.conversation_id)) ? <ApprovalDot/>
                        : unread ? <UnreadDot/> : null}
                </button>
            </div></ContextMenuTrigger>
            <ContextMenuContent>
                <ContextMenuItem variant="destructive" onSelect={() => onDeleteGroup(group)}>删除群聊</ContextMenuItem>
            </ContextMenuContent>
        </ContextMenu> };
    });
    const assistantRows = contacts.map(assistant => {
        const preview = previews.find(p => p.assistantId === assistant.id);
        return { id: `assistant:${assistant.id}`, label: assistant.name, content: <AssistantItemContextMenu assistant={assistant} onAction={onAssistantAction} canReset><button onClick={() => { acknowledge?.(assistant.id); onSelect(assistant); }} disabled={!ready}
            aria-pressed={selected === assistant.id} className="butler-contact">
            <ButlerAvatar id={assistant.id} size={48} compact figure animated={state(assistant.id) !== "idle"} state={state(assistant.id)} />
            <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{assistant.name}</span>
                <span className="block truncate text-xs text-muted-foreground">{visuals.get(assistant.id)?.label || preview?.preview || "尚未对话"}</span>
            </span>
            {approvals.has(assistant.id) ? <ApprovalDot/>
                : state(assistant.id) === "alerting" ? <UnreadDot/> : null}
        </button></AssistantItemContextMenu> };
    });
    return <>
        <div className="flex min-h-12 shrink-0 flex-wrap items-center justify-between gap-1.5 px-3 py-1">
            <div className="flex items-center gap-1.5">
                <Button size="icon" variant="outline" className="size-8 rounded-full" aria-label="总管家设置" title="总管家设置"
                    onClick={onOpenSettings} data-aipp-slot="butler-main-open-settings"><Settings/></Button>
                <AppUpdateNotice rounded />
                {onOpenProactive && <Button size="icon" variant="outline" className="size-8 rounded-full" aria-label="主动整理" title="主动整理：跟进事项与整理手记"
                    disabled={!ready} onClick={onOpenProactive} data-aipp-slot="butler-proactive-open"><CalendarHeart/></Button>}
            </div>
            <div className="flex items-center gap-1.5">
                <Button size="icon" variant="outline" className="size-8 rounded-full" aria-label="搜索管家消息" title="搜索管家消息"
                    disabled={!ready} onClick={onSearch} data-aipp-slot="butler-search"><Search/></Button>
                <Button size="icon" variant="outline" className="size-8 rounded-full" aria-label="发起群聊" title="发起群聊"
                    disabled={!ready} onClick={onCreateGroup} data-aipp-slot="butler-create-group"><UsersRound/></Button>
                <AddAssistantDialog assistantTypes={assistantTypes} onAssistantAdded={onRefresh} pluginList={plugins}
                    triggerButtonProps={{variant:"outline",size:"icon",className:"size-8 rounded-full",title:"添加助手","aria-label":"添加助手"}}/>
            </div>
        </div>
        <div className="mb-4 flex shrink-0 justify-center">
            <button onClick={() => { acknowledge?.(0); onSelect(); }} aria-pressed={selected === undefined && selectedGroup === undefined}
                className="flex max-w-full flex-col items-center gap-1.5 rounded-xl px-5 py-2 text-xs font-medium hover:bg-muted aria-pressed:bg-muted">
                <ButlerAvatar id={0} chief size={66} compact figure animated={state(0) !== "idle"} state={state(0)}/>
                <span className="inline-flex max-w-full items-center gap-1.5">{chief}{approvals.has(0) ? <ApprovalDot/> : null}</span>
                {visuals.get(0)?.label && <span className="max-w-full truncate text-xs text-muted-foreground">{visuals.get(0)?.label}</span>}
            </button>
        </div>
        <nav aria-label="助手联系人" className="min-h-0 flex-1 overflow-y-auto px-2 pb-3">
            {error && <button onClick={onRefresh} className="px-2 py-1 text-xs text-destructive">刷新联系人</button>}
            <ButlerSortableList rows={[...groupRows, ...assistantRows]}/>
        </nav>
    </>;
}
