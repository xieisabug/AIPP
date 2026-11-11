import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2, Trash2, UserMinus, UserPlus, Users } from "lucide-react";
import { toast } from "sonner";
import IconButton from "@/components/IconButton";
import InputArea from "@/components/conversation/InputArea";
import type { MentionCandidate } from "@/components/conversation/AssistantCompletionList";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import {
    AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
    AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import type { AssistantListItem } from "@/data/Assistant";
import { type ButlerGroupDetail, type ButlerGroupMember, type ButlerGroupMessage } from "@/data/ButlerGroup";
import * as groups from "@/services/butlerGroupService";
import { ConversationStatsDialog } from "@/components/token-statistics";
import ButlerAvatar from "./ButlerAvatar";
import ButlerGroupAvatar from "./ButlerGroupAvatar";
import ButlerGroupMemberPicker from "./ButlerGroupMemberPicker";
import ButlerGroupTranscript from "./ButlerGroupTranscript";
import ButlerMemoryPanel from "./ButlerMemoryPanel";
import { ButlerQuoteChip, ButlerQuoteContext, type ButlerQuoteTarget } from "./ButlerQuote";
import { groupReadKey, mentionTargets, mergeMessages } from "./butlerGroupModel";

interface Props {
    groupId: number;
    chiefName: string;
    assistants: AssistantListItem[];
    onDeleted: () => void;
}

/** 打开群聊时一次取回全部历史（与私聊一样交给虚拟列表），不再分页“查看更早的消息”。 */
const FULL_HISTORY_LIMIT = 100_000;

export default function ButlerGroupChat({ groupId, chiefName, assistants, onDeleted }: Props) {
    const [detail, setDetail] = useState<ButlerGroupDetail | null>(null);
    const [older, setOlder] = useState<ButlerGroupMessage[]>([]);
    const [loadError, setLoadError] = useState("");
    const [draft, setDraft] = useState("");
    // 引用与私聊共用同一套组件：悬浮条/右键菜单的“引用回复”写入这里，输入框上方显示引用标签。id 是群消息序号。
    const [quote, setQuote] = useState<ButlerQuoteTarget | null>(null);
    const quoteState = useMemo(() => ({ quote, setQuote }), [quote]);
    const [sending, setSending] = useState(false);
    const [stopping, setStopping] = useState(false);
    const [panelOpen, setPanelOpen] = useState(false);
    const [inviteOpen, setInviteOpen] = useState(false);
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [nameDraft, setNameDraft] = useState("");
    const pendingPost = useRef<{ signature: string; key: string } | null>(null);
    const historyLoaded = useRef(false);
    const revision = useRef(0);

    const refresh = useCallback(async () => {
        const version = ++revision.current;
        try {
            const next = await groups.getButlerGroup(groupId);
            // 首次打开时把最新一页之前的历史一并取回，再交给列表一次性展示并贴底，避免先显示再往上插入导致跳动。
            let earlier: ButlerGroupMessage[] | null = null;
            if (!historyLoaded.current) {
                earlier = next.has_more && next.messages.length
                    ? await groups.listOlderButlerGroupMessages(groupId, next.messages[0].seq, FULL_HISTORY_LIMIT)
                    : [];
            }
            if (version !== revision.current) return;
            if (earlier) {
                historyLoaded.current = true;
                setOlder(earlier);
            }
            setDetail(next);
            setLoadError("");
        } catch (error) {
            if (version === revision.current) setLoadError(String(error));
        }
    }, [groupId]);

    useEffect(() => {
        let timer: ReturnType<typeof setTimeout> | undefined;
        void refresh();
        const subscription = groups.onButlerGroupChanged(changed => {
            if (changed !== groupId) return;
            clearTimeout(timer);
            timer = setTimeout(() => void refresh(), 80);
        });
        return () => {
            clearTimeout(timer);
            ++revision.current;
            void subscription.then(stop => stop()).catch(() => {});
        };
    }, [groupId, refresh]);

    const messages = useMemo(() => mergeMessages(older, detail?.messages ?? []), [older, detail?.messages]);
    const lastSeq = messages.length ? messages[messages.length - 1].seq : 0;
    useEffect(() => {
        if (lastSeq) localStorage.setItem(groupReadKey(groupId), String(lastSeq));
    }, [groupId, lastSeq]);

    const members = useMemo(() => detail?.members ?? [], [detail?.members]);
    const running = useMemo(() => {
        const ids = new Set((detail?.turns ?? []).filter(turn => turn.state === "running").map(turn => turn.member_id));
        return members.filter(member => ids.has(member.id));
    }, [detail?.turns, members]);
    const replying = !!detail && (detail.routing || detail.turns.length > 0);

    // @ 候选交给通用输入框过滤（支持拼音/首字母），这里只提供本群成员与头像。
    const mentionCandidates = useMemo<MentionCandidate[]>(() => mentionTargets(members).map(target => ({
        id: target.id,
        name: target.name,
        description: target.description,
        icon: target.member
            ? <ButlerAvatar id={target.member.assistant_id} chief={target.member.is_chief} size={20} figure />
            : <Users className="size-4 text-muted-foreground" />,
    })), [members]);

    const send = async () => {
        const body = draft.trim();
        if (!body || sending) return;
        const replyTo = quote?.id ?? null;
        const signature = JSON.stringify([body, replyTo]);
        if (pendingPost.current?.signature !== signature) {
            pendingPost.current = { signature, key: crypto.randomUUID() };
        }
        setSending(true);
        try {
            const message = await groups.postButlerGroupMessage(groupId, pendingPost.current.key, body, replyTo);
            pendingPost.current = null;
            setDraft("");
            setQuote(null);
            // 本地先并入自己的发言：列表看到新的“末尾用户消息”，播放与私聊相同的发送滑动并贴底。
            setDetail(previous => previous ? { ...previous, routing: true, messages: mergeMessages(previous.messages, [message]) } : previous);
            void refresh();
        } catch (error) {
            // 草稿保留，重试时沿用同一个 request key，后端不会重复入群
            toast.error(`发送群消息失败：${error}`);
        } finally {
            setSending(false);
        }
    };

    const stop = async () => {
        setStopping(true);
        try {
            const result = await groups.stopButlerGroupReplies(groupId);
            if (result.errors.length) toast.error(result.errors.join("\n"));
        } catch (error) {
            toast.error(`停止回复失败：${error}`);
        } finally {
            setStopping(false);
            void refresh();
        }
    };

    // 与私聊同一套发送键语义：有内容就发送（成员回复中也可以插话）；输入为空且有人在回复时，按钮/回车是停止。
    const handleSend = () => {
        if (draft.trim()) void send();
        else if (replying && !stopping) void stop();
    };

    const saveName = async () => {
        const name = nameDraft.trim();
        if (!detail || !name || name === detail.name) return;
        try {
            await groups.renameButlerGroup(groupId, name);
            await refresh();
        } catch (error) {
            toast.error(String(error));
        }
    };

    const removeMember = async (member: ButlerGroupMember) => {
        try {
            await groups.removeButlerGroupMember(groupId, member.id);
            await refresh();
        } catch (error) {
            toast.error(String(error));
        }
    };

    const deleteGroup = async () => {
        try {
            await groups.deleteButlerGroup(groupId);
            localStorage.removeItem(groupReadKey(groupId));
            onDeleted();
        } catch (error) {
            toast.error(String(error));
        }
    };

    if (!detail) {
        return <div className="flex h-full items-center justify-center p-6 text-sm text-muted-foreground">
            {loadError ? <div className="space-y-3 text-center"><p className="whitespace-pre-wrap text-destructive">{loadError}</p>
                <Button variant="outline" size="sm" onClick={() => void refresh()}>重试</Button></div> : "正在打开群聊…"}
        </div>;
    }

    const discussion = detail.turns.filter(turn => turn.rounds > 1).sort((a, b) => b.round - a.round)[0];
    const typing = running.length ? `${running.map(member => member.name).join("、")} 正在输入…` : "";
    const subtitle = discussion
        ? [`讨论中 · 第 ${discussion.round}/${discussion.rounds} 轮`, typing].filter(Boolean).join(" · ")
        : typing || `${members.length} 位成员`;

    return <div className="flex h-full min-h-0 flex-col" data-aipp-slot="butler-group-chat">
        <div className="grid flex-none grid-cols-[1fr_auto_1fr] items-center gap-2 border-b border-border px-3 py-2" data-aipp-slot="butler-group-header">
            <div />
            <button type="button" onClick={() => { setNameDraft(detail.name); setPanelOpen(true); }}
                className="flex min-w-0 max-w-[26rem] items-center gap-2 rounded-lg px-2 py-1 text-left transition-colors hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
                title="群聊信息">
                <ButlerGroupAvatar members={members} size={28} />
                <span className="flex min-w-0 flex-col">
                    <span className="truncate text-sm font-medium leading-4">{detail.name}</span>
                    <span className={`truncate text-[11px] leading-[14px] ${running.length || discussion ? "text-foreground/70" : "text-muted-foreground"}`}>{subtitle}</span>
                </span>
            </button>
            <div className="flex items-center justify-end gap-1">
                <ConversationStatsDialog groupId={groupId} />
                <IconButton icon={<Users className="h-4 w-4 text-icon" />} border title="群成员"
                    onClick={() => { setNameDraft(detail.name); setPanelOpen(true); }} />
            </div>
        </div>

        <ButlerQuoteContext.Provider value={quoteState}>
        {/* 消息列表与私聊同一套（见 ButlerGroupTranscript）：输入框悬浮在列表底部之上，尾部空间由列表留出。 */}
        <ButlerGroupTranscript groupId={groupId} messages={messages} members={members} running={running} />

        {/* 群聊消息只有正文和引用，暂不支持附件，所以不传附件能力。 */}
        <InputArea
            inputText={draft}
            setInputText={setDraft}
            handleSend={handleSend}
            aiIsResponsing={replying}
            placement="bottom"
            sendButtonIcon={sending || stopping ? <Loader2 size={20} className="animate-spin text-action-foreground" /> : undefined}
            mentionCandidates={mentionCandidates}
            placeholder="发消息，输入 @ 可以点名"
            ariaLabel="群聊消息"
            topChip={quote ? <ButlerQuoteChip quote={quote} onClear={() => setQuote(null)} /> : undefined}
        />
        </ButlerQuoteContext.Provider>

        <Sheet open={panelOpen} onOpenChange={setPanelOpen}>
            <SheetContent className="w-80 gap-0 sm:max-w-sm">
                <SheetHeader>
                    <SheetTitle>群聊信息</SheetTitle>
                    <SheetDescription>成员在群里有各自独立的上下文，不会带入私聊内容。</SheetDescription>
                </SheetHeader>
                <div className="flex min-h-0 flex-1 flex-col gap-5 overflow-y-auto px-4 pb-4">
                    <label className="space-y-1.5">
                        <span className="text-xs text-muted-foreground">群聊名称</span>
                        <Input value={nameDraft} maxLength={60} onChange={event => setNameDraft(event.target.value)}
                            onBlur={() => void saveName()} onKeyDown={event => { if (event.key === "Enter") event.currentTarget.blur(); }} />
                    </label>
                    <section className="space-y-1">
                        <div className="flex items-center justify-between">
                            <span className="text-xs text-muted-foreground">成员 · {members.length}</span>
                            <Button variant="ghost" size="sm" className="h-7 gap-1 px-2 text-xs" onClick={() => setInviteOpen(true)}>
                                <UserPlus className="size-3.5" />邀请
                            </Button>
                        </div>
                        {members.map(member => <div key={member.id} className="group flex items-center gap-2.5 rounded-lg px-1.5 py-1.5 hover:bg-muted">
                            <ButlerAvatar id={member.assistant_id} chief={member.is_chief} size={28} figure />
                            <span className="min-w-0 flex-1 truncate text-sm">{member.name}</span>
                            {members.length > 1 && <button type="button" title={`将 ${member.name} 移出群聊`} aria-label={`将 ${member.name} 移出群聊`}
                                className="rounded p-1 text-muted-foreground opacity-0 transition-opacity hover:bg-background hover:text-foreground group-hover:opacity-100 focus-visible:opacity-100"
                                onClick={() => void removeMember(member)}><UserMinus className="size-3.5" /></button>}
                        </div>)}
                    </section>
                    <ButlerMemoryPanel conversationId={groupId} scope="group" />
                    <Button variant="ghost" className="mt-auto justify-start gap-2 text-destructive hover:text-destructive" onClick={() => setConfirmDelete(true)}>
                        <Trash2 className="size-4" />删除群聊
                    </Button>
                </div>
            </SheetContent>
        </Sheet>

        <ButlerGroupMemberPicker open={inviteOpen} onOpenChange={setInviteOpen} mode="invite" chiefName={chiefName} assistants={assistants}
            existingAssistantIds={members.map(member => member.assistant_id)} chiefInGroup={members.some(member => member.is_chief)}
            onConfirm={async result => {
                await groups.addButlerGroupMembers(groupId, result.assistantIds, result.includeChief);
                await refresh();
            }} />

        <AlertDialog open={confirmDelete} onOpenChange={setConfirmDelete}>
            <AlertDialogContent>
                <AlertDialogHeader>
                    <AlertDialogTitle>删除群聊「{detail.name}」？</AlertDialogTitle>
                    <AlertDialogDescription>群消息和成员在本群的执行记录都会删除，成员的私聊不受影响。正在进行的回复会先停止。</AlertDialogDescription>
                </AlertDialogHeader>
                <AlertDialogFooter>
                    <AlertDialogCancel>取消</AlertDialogCancel>
                    <AlertDialogAction className="bg-destructive text-white hover:bg-destructive/90" onClick={() => void deleteGroup()}>删除</AlertDialogAction>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    </div>;
}
