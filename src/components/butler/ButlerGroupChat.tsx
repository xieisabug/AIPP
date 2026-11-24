import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Loader2, Trash2, UserMinus, UserPlus, Users } from "lucide-react";
import { toast } from "sonner";
import IconButton from "@/components/IconButton";
import InputArea from "@/components/conversation/InputArea";
import type { MentionCandidate } from "@/components/conversation/AssistantCompletionList";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { afterSendMotion } from "@/components/conversation/sendMotion";
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
import { useFocusedWindow } from "@/hooks/useFocusedWindow";
import { FloatingWindowControls, WINDOW_HEADER_HEIGHT, WindowControls, dragRegionProps, useFramelessTitleBar } from "@/components/window/WindowTitleBar";
import { groupReadKey, mentionTargets, mergeMessages } from "./butlerGroupModel";

interface Props {
    groupId: number;
    sourceSeq?: number;
    onSourceLocated?: () => void;
    chiefName: string;
    assistants: AssistantListItem[];
    onDeleted: () => void;
}

/** 打开群聊时一次取回全部历史（与私聊一样交给虚拟列表），不再分页“查看更早的消息”。 */
const HISTORY_PAGE_SIZE = 200;

export default function ButlerGroupChat({ groupId, chiefName, assistants, onDeleted, sourceSeq, onSourceLocated }: Props) {
    const [detail, setDetail] = useState<ButlerGroupDetail | null>(null);
    const [loadError, setLoadError] = useState("");
    const [draft, setDraft] = useState("");
    // 引用与私聊共用同一套组件：悬浮条/右键菜单的“引用回复”写入这里，输入框上方显示引用标签。id 是群消息序号。
    const [quote, setQuote] = useState<ButlerQuoteTarget | null>(null);
    const quoteState = useMemo(() => ({ quote, setQuote }), [quote]);
    const [sending, setSending] = useState(false);
    const [stopping, setStopping] = useState(false);
    const [panelOpen, setPanelOpen] = useState(false);
    // 无系统标题栏时头部空白处可拖动窗口；群聊头部贴着窗口右上角，Windows 窗口控制按钮排在末尾
    const framelessTitleBar = useFramelessTitleBar();
    const drag = dragRegionProps(framelessTitleBar);
    const [inviteOpen, setInviteOpen] = useState(false);
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [nameDraft, setNameDraft] = useState("");
    const pendingPost = useRef<{ signature: string; key: string } | null>(null);
    const [localPost, setLocalPost] = useState<ButlerGroupMessage>();
    const historyLoaded = useRef(false);
    const loadedThrough = useRef(0);
    const revision = useRef(0);

    const refresh = useCallback(async (replaceHistory = false) => {
        if (replaceHistory) historyLoaded.current = false;
        const loadFullHistory = !historyLoaded.current;
        const version = ++revision.current;
        try {
            const next = await groups.getButlerGroup(groupId);
            const pages: ButlerGroupMessage[][] = [next.messages];
            let before = next.messages[0]?.seq ?? 0;
            while (next.has_more && before > 0 && (loadFullHistory || before > loadedThrough.current + 1)) {
                if (version !== revision.current) return;
                const page = await groups.listOlderButlerGroupMessages(groupId, before, HISTORY_PAGE_SIZE);
                if (!page.length || page[0].seq >= before) break;
                pages.push(page);
                before = page[0].seq;
                if (page.length < HISTORY_PAGE_SIZE) break;
            }
            if (version !== revision.current) return;
            const incoming = pages.reverse().flat();
            historyLoaded.current = true;
            loadedThrough.current = next.messages.at(-1)?.seq ?? loadedThrough.current;
            setDetail(previous => ({ ...next, messages: mergeMessages(loadFullHistory ? [] : previous?.messages ?? [], incoming) }));
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
            timer = setTimeout(() => afterSendMotion(() => void refresh()), 80);
        });
        const sync = listen("conversation_data_changed", () => {
            clearTimeout(timer);
            void refresh(true);
        });
        return () => {
            void sync.then(stop => stop()).catch(console.warn);
            clearTimeout(timer);
            ++revision.current;
            void subscription.then(stop => stop()).catch(() => {});
        };
    }, [groupId, refresh]);

    const messages = detail?.messages ?? [];
    const displayMessages = useMemo(() =>
        localPost?.group_id === groupId && !messages.some(row => row.request_key === localPost.request_key)
            ? [...messages, localPost] : messages, [localPost, groupId, messages]);
    const focused = useFocusedWindow();
    const lastSeq = messages.length ? messages[messages.length - 1].seq : 0;
    const marked = useRef(new Set<number>());
    useEffect(() => { marked.current = new Set(); }, [groupId]);
    useEffect(() => {
        if (!lastSeq || !focused || !document.hasFocus() || document.hidden) return;
        const receipts = marked.current;
        const visible = messages.filter(m => m.sender_member != null && m.kind === "message" && !receipts.has(m.seq)).map(m => m.seq);
        if (!visible.length) return;
        for (const seq of visible) receipts.add(seq);
        void groups.markRepliesRead(groupId, lastSeq, true, visible).catch(error => {
            for (const seq of visible) receipts.delete(seq);
            console.warn("记录群聊已读失败", error);
        });
    }, [groupId, messages, lastSeq, focused]);

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

    const send = async (body: string) => {
        if (!body || sending) return;
        const replyTo = quote?.id ?? null;
        const signature = JSON.stringify([body, replyTo]);
        if (pendingPost.current?.signature !== signature) {
            pendingPost.current = { signature, key: crypto.randomUUID() };
        }
        const requestKey = pendingPost.current.key;
        setLocalPost({
            group_id: groupId, seq: Number.MAX_SAFE_INTEGER, request_key: requestKey, sendStatus: "sending",
            sender_member: null, sender_assistant: null, sender_name: "", kind: "message", body,
            reply_to: replyTo, created_at: new Date().toISOString(), source_conversation_id: null, source_message_id: null,
        });
        setSending(true);
        setDraft("");
        try {
            const message = await groups.postButlerGroupMessage(groupId, requestKey, body, replyTo);
            pendingPost.current = null;
            setQuote(current => current === quote ? null : current);
            // 用同一 request_key 将正式序号交给已经上屏的气泡；等发送动画结束再换，避免整表重渲染掉帧。
            afterSendMotion(() => {
                setLocalPost(current => current?.request_key === requestKey ? undefined : current);
                setDetail(previous => previous ? { ...previous, routing: true, messages: mergeMessages(previous.messages, [message]) } : previous);
                void refresh();
            });
        } catch (error) {
            setLocalPost(current => current && { ...current, sendStatus: "failed", sendError: String(error) });
            setDraft(current => current || body);
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
    const handleSend = (emptyPrompt?: string) => {
        const body = draft.trim() || (!replying && emptyPrompt);
        if (body) void send(body);
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
        return <div className="flex h-full select-none items-center justify-center p-6 text-sm text-muted-foreground" {...drag}>
            <FloatingWindowControls centerY={WINDOW_HEADER_HEIGHT / 2} right={12} />
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
        <div className="grid flex-none select-none grid-cols-[1fr_auto_1fr] items-center gap-2 border-b border-border px-3" style={{ height: WINDOW_HEADER_HEIGHT }}
            {...drag} data-aipp-slot="butler-group-header">
            <div className="self-stretch" {...drag} />
            <button type="button" onClick={() => { setNameDraft(detail.name); setPanelOpen(true); }}
                className="flex min-w-0 max-w-[26rem] items-center gap-2 rounded-lg px-2 py-1 text-left transition-colors hover:bg-muted focus-visible:outline-2 focus-visible:outline-ring"
                title="群聊信息">
                <ButlerGroupAvatar members={members} size={28} />
                <span className="flex min-w-0 flex-col">
                    <span className="truncate text-sm font-medium leading-4">{detail.name}</span>
                    <span className={`truncate text-[11px] leading-[14px] ${running.length || discussion ? "text-foreground/70" : "text-muted-foreground"}`}>{subtitle}</span>
                </span>
            </button>
            <div className="flex items-center justify-end gap-1 self-stretch" {...drag}>
                <ConversationStatsDialog groupId={groupId} />
                <IconButton icon={<Users className="h-4 w-4 text-icon" />} ghost title="群成员"
                    onClick={() => { setNameDraft(detail.name); setPanelOpen(true); }} />
                <WindowControls className="ml-2" />
            </div>
        </div>

        <ButlerQuoteContext.Provider value={quoteState}>
        {/* 消息列表与私聊同一套（见 ButlerGroupTranscript）：输入框悬浮在列表底部之上，尾部空间由列表留出。 */}
        <ButlerGroupTranscript historyLoaded={detail.id === groupId && historyLoaded.current} groupId={groupId}
            sourceSeq={sourceSeq} onSourceLocated={onSourceLocated}
            messages={displayMessages} members={members} running={running} turns={detail.interaction_turns} />

        {/* 群聊消息只有正文和引用，暂不支持附件，所以不传附件能力。 */}
        <InputArea
            permissionConversationIds={members.map(member => member.conversation_id)}
            inputText={draft}
            setInputText={setDraft}
            handleSend={handleSend}
            aiIsResponsing={replying}
            placement="bottom"
            sendButtonIcon={sending || stopping ? <Loader2 size={20} className="animate-spin text-action-foreground" /> : undefined}
            commandMode="butler"
            onContextCommand={async command => {
                if (!members.length) throw new Error("群聊没有成员");
                const failures: string[] = [];
                for (const member of members) {
                    try {
                        await invoke("run_conversation_context_command", { conversationId: member.conversation_id, command });
                    } catch (error) {
                        failures.push(`${member.name}：${String(error)}`);
                    }
                }
                if (failures.length) throw new Error(failures.join("；"));
                toast.success(command === "reload" ? "群成员会话已重新加载" : "已向群成员会话执行压缩命令");
            }}
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
            <AlertDialogContent className="flex flex-col gap-0 overflow-hidden p-0 sm:max-w-md">
                <AlertDialogHeader className="border-b border-border px-4 py-3 text-left sm:px-5">
                    <AlertDialogTitle className="text-base font-semibold">删除群聊「{detail.name}」？</AlertDialogTitle>
                </AlertDialogHeader>
                <div className="px-4 py-4 sm:px-5">
                    <AlertDialogDescription className="text-sm leading-relaxed text-muted-foreground">
                        群消息和成员在本群的执行记录都会删除，成员的私聊不受影响。正在进行的回复会先停止。
                    </AlertDialogDescription>
                </div>
                <AlertDialogFooter className="flex-row items-center justify-end gap-2 border-t border-border bg-background px-4 pt-2.5 pb-[calc(0.625rem+env(safe-area-inset-bottom))] sm:px-5">
                    <AlertDialogCancel className="min-h-11 px-4 sm:min-h-9">取消</AlertDialogCancel>
                    <AlertDialogAction className="min-h-11 bg-destructive px-5 text-white hover:bg-destructive/90 sm:min-h-9" onClick={() => void deleteGroup()}>删除</AlertDialogAction>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    </div>;
}
