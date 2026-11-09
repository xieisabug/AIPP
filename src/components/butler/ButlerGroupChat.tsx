import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { listen } from "@tauri-apps/api/event";
import { ArrowUp, Loader2, Square, Trash2, UserMinus, UserPlus, Users } from "lucide-react";
import { toast } from "sonner";
import IconButton from "@/components/IconButton";
import { AskUserQuestionCard } from "@/components/InlineInteractionCards";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import {
    AlertDialog, AlertDialogAction, AlertDialogCancel, AlertDialogContent, AlertDialogDescription,
    AlertDialogFooter, AlertDialogHeader, AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { useAskUserQuestion } from "@/hooks/useInlineInteraction";
import { useAcpElicitation } from "@/hooks/useAcpElicitation";
import type { AssistantListItem } from "@/data/Assistant";
import { type ButlerGroupDetail, type ButlerGroupMember, type ButlerGroupMessage } from "@/data/ButlerGroup";
import * as groups from "@/services/butlerGroupService";
import ButlerAvatar from "./ButlerAvatar";
import ButlerGroupAvatar from "./ButlerGroupAvatar";
import ButlerGroupMemberPicker from "./ButlerGroupMemberPicker";
import ButlerGroupMessageRow from "./ButlerGroupMessageRow";
import { ButlerQuoteChip, ButlerQuoteContext, type ButlerQuoteTarget } from "./ButlerQuote";
import { BUTLER_PROGRESS_EVENT, type ButlerProgressSummary } from "./butlerProgress";
import {
    applyMention, findMentionQuery, formatDivider, groupReadKey, mentionOptions, mergeMessages,
    showTimeDivider, type MentionQuery,
} from "./butlerGroupModel";

interface Props {
    groupId: number;
    chiefName: string;
    assistants: AssistantListItem[];
    onDeleted: () => void;
}

/** 只显示这次回复开始之后的进展摘要，避免把上一轮的旧摘要当成当前进展。 */
function useLiveProgress(conversationId: number) {
    const [text, setText] = useState("");
    useEffect(() => {
        let disposed = false;
        const unlisten = listen<ButlerProgressSummary>(BUTLER_PROGRESS_EVENT, ({ payload }) => {
            if (!disposed && payload.conversation_id === conversationId) setText(payload.text);
        });
        return () => {
            disposed = true;
            void unlisten.then(stop => stop()).catch(() => {});
        };
    }, [conversationId]);
    return text;
}

function TypingRow({ member }: { member: ButlerGroupMember }) {
    const progress = useLiveProgress(member.conversation_id);
    return <div className="flex items-start gap-2.5" role="status" aria-label={`${member.name} 正在输入`} data-aipp-slot="butler-group-typing">
        <ButlerAvatar id={member.assistant_id} chief={member.is_chief} size={32} figure animated state="thinking" />
        <div className="min-w-0">
            <div className="mb-1 text-xs text-muted-foreground">{member.name}</div>
            <div className="butler-peer-bubble flex items-center gap-2 text-muted-foreground">
                <span className="butler-typing-dots" aria-hidden="true"><i /><i /><i /></span>
                {progress && <span className="truncate text-xs">{progress}</span>}
            </div>
        </div>
    </div>;
}

/** 成员在执行中向用户提的问题，显示在群聊里，答案回到这位成员的原执行。 */
function MemberQuestion({ member }: { member: ButlerGroupMember }) {
    const question = useAskUserQuestion({ conversationId: member.conversation_id });
    const elicitation = useAcpElicitation({ conversationId: member.conversation_id });
    if (!question.isDialogOpen && !elicitation.questionRequest && !elicitation.decisionError) return null;
    return <div className="flex items-start gap-2.5" data-aipp-slot="butler-group-question">
        <ButlerAvatar id={member.assistant_id} chief={member.is_chief} size={32} figure />
        <div className="min-w-0 flex-1 max-w-[min(80%,48rem)]">
            <div className="mb-1 text-xs text-muted-foreground">{member.name} 想问你</div>
            {question.isDialogOpen && <AskUserQuestionCard request={question.pendingRequest} isOpen={question.isDialogOpen}
                viewMode={question.viewMode} completedAnswers={question.completedAnswers} readOnly={question.readOnly}
                onSubmit={question.handleSubmit} onCancel={question.handleCancel} />}
            {elicitation.questionRequest && <AskUserQuestionCard request={elicitation.questionRequest} isOpen viewMode="questionnaire"
                errorMessage={elicitation.decisionError} onSubmit={elicitation.handleSubmit} onCancel={elicitation.handleCancel} />}
            {!elicitation.questionRequest && elicitation.decisionError &&
                <p role="alert" className="text-sm text-destructive">{elicitation.decisionError}</p>}
        </div>
    </div>;
}

export default function ButlerGroupChat({ groupId, chiefName, assistants, onDeleted }: Props) {
    const [detail, setDetail] = useState<ButlerGroupDetail | null>(null);
    const [older, setOlder] = useState<ButlerGroupMessage[]>([]);
    const [hasMoreOlder, setHasMoreOlder] = useState<boolean | null>(null);
    const [loadError, setLoadError] = useState("");
    const [draft, setDraft] = useState("");
    // 引用与私聊共用同一套组件：悬浮条/右键菜单的“引用回复”写入这里，输入框上方显示引用标签。id 是群消息序号。
    const [quote, setQuote] = useState<ButlerQuoteTarget | null>(null);
    const quoteState = useMemo(() => ({ quote, setQuote }), [quote]);
    const [sending, setSending] = useState(false);
    const [stopping, setStopping] = useState(false);
    const [inputError, setInputError] = useState("");
    const [panelOpen, setPanelOpen] = useState(false);
    const [inviteOpen, setInviteOpen] = useState(false);
    const [confirmDelete, setConfirmDelete] = useState(false);
    const [nameDraft, setNameDraft] = useState("");
    const [mention, setMention] = useState<MentionQuery | null>(null);
    const [mentionIndex, setMentionIndex] = useState(0);
    const pendingPost = useRef<{ signature: string; key: string } | null>(null);
    const listRef = useRef<HTMLDivElement>(null);
    const inputRef = useRef<HTMLTextAreaElement>(null);
    const stickToBottom = useRef(true);
    const revision = useRef(0);

    const refresh = useCallback(async () => {
        const version = ++revision.current;
        try {
            const next = await groups.getButlerGroup(groupId);
            if (version !== revision.current) return;
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

    // 停在底部时，新消息和“正在输入”出现后继续跟随到底部；用户上翻查看历史时不打扰。
    useLayoutEffect(() => {
        const list = listRef.current;
        if (list && stickToBottom.current) list.scrollTop = list.scrollHeight;
    }, [messages.length, running.length, detail?.id]);
    const onScroll = () => {
        const list = listRef.current;
        if (!list) return;
        stickToBottom.current = list.scrollHeight - list.scrollTop - list.clientHeight < 80;
    };

    const loadOlder = async () => {
        const first = messages[0];
        if (!first) return;
        try {
            const page = await groups.listOlderButlerGroupMessages(groupId, first.seq);
            const list = listRef.current;
            const previousHeight = list?.scrollHeight ?? 0;
            setOlder(previous => mergeMessages(page, previous));
            setHasMoreOlder(page.length >= 80);
            requestAnimationFrame(() => {
                if (list) list.scrollTop += list.scrollHeight - previousHeight;
            });
        } catch (error) {
            toast.error(`读取更早的群消息失败：${error}`);
        }
    };

    const options = useMemo(() => mention ? mentionOptions(members, mention.query) : [], [mention, members]);
    const updateMention = (text: string, caret: number) => {
        const next = findMentionQuery(text, caret);
        setMention(next);
        setMentionIndex(0);
    };
    const chooseMention = (label: string) => {
        const input = inputRef.current;
        if (!mention || !input) return;
        const caret = input.selectionStart ?? draft.length;
        const next = applyMention(draft, mention, caret, label);
        setDraft(next.text);
        setMention(null);
        requestAnimationFrame(() => {
            input.focus();
            input.setSelectionRange(next.caret, next.caret);
        });
    };

    const send = async () => {
        const body = draft.trim();
        if (!body || sending) return;
        const replyTo = quote?.id ?? null;
        const signature = JSON.stringify([body, replyTo]);
        if (pendingPost.current?.signature !== signature) {
            pendingPost.current = { signature, key: crypto.randomUUID() };
        }
        setSending(true);
        setInputError("");
        try {
            const message = await groups.postButlerGroupMessage(groupId, pendingPost.current.key, body, replyTo);
            pendingPost.current = null;
            setDraft("");
            setQuote(null);
            setMention(null);
            stickToBottom.current = true;
            setDetail(previous => previous ? { ...previous, routing: true, messages: mergeMessages(previous.messages, [message]) } : previous);
            void refresh();
        } catch (error) {
            setInputError(String(error));
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

    const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
        if (event.nativeEvent.isComposing) return;
        if (mention && options.length) {
            if (event.key === "ArrowDown" || event.key === "ArrowUp") {
                event.preventDefault();
                setMentionIndex(index => (index + (event.key === "ArrowDown" ? 1 : options.length - 1)) % options.length);
                return;
            }
            if (event.key === "Enter" || event.key === "Tab") {
                event.preventDefault();
                chooseMention(options[Math.min(mentionIndex, options.length - 1)].label);
                return;
            }
            if (event.key === "Escape") {
                event.preventDefault();
                setMention(null);
                return;
            }
        }
        if (event.key === "Enter" && !event.shiftKey) {
            event.preventDefault();
            void send();
        }
    };

    useEffect(() => {
        const input = inputRef.current;
        if (!input) return;
        input.style.height = "auto";
        input.style.height = `${Math.min(input.scrollHeight, 200)}px`;
    }, [draft]);

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

    const byId = new Map(members.map(member => [member.id, member]));
    const quotedBySeq = new Map(messages.map(message => [message.seq, message]));
    const canLoadOlder = hasMoreOlder ?? detail.has_more;
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
            <div className="flex items-center justify-end">
                <IconButton icon={<Users className="h-4 w-4 text-icon" />} border title="群成员"
                    onClick={() => { setNameDraft(detail.name); setPanelOpen(true); }} />
            </div>
        </div>

        <ButlerQuoteContext.Provider value={quoteState}>
        <div ref={listRef} onScroll={onScroll} className="min-h-0 flex-1 overflow-y-auto px-5 py-4" aria-label="群聊记录" data-aipp-slot="butler-group-messages">
            <div className="mx-auto flex max-w-4xl flex-col gap-4">
                {canLoadOlder && <button type="button" className="self-center rounded-full px-3 py-1 text-xs text-muted-foreground hover:bg-muted"
                    onClick={() => void loadOlder()}>查看更早的消息</button>}
                {messages.map((message, index) => {
                    const divider = showTimeDivider(message, messages[index - 1]);
                    const key = `${message.seq}`;
                    const time = divider && <div className="py-1 text-center text-[11px] text-muted-foreground">{formatDivider(message.created_at)}</div>;
                    if (message.kind !== "message") {
                        return <div key={key}>
                            {time}
                            <div className="flex justify-center" data-aipp-slot={`butler-group-${message.kind}`}>
                                <span className={`max-w-[80%] whitespace-pre-wrap break-words rounded-lg px-3 py-1 text-center text-xs ${message.kind === "error" ? "bg-destructive/5 text-destructive" : "text-muted-foreground"}`}>
                                    {message.body}
                                </span>
                            </div>
                        </div>;
                    }
                    return <div key={key}>
                        {time}
                        <ButlerGroupMessageRow groupId={groupId} message={message}
                            quoted={message.reply_to ? quotedBySeq.get(message.reply_to) : undefined}
                            sender={message.sender_member ? byId.get(message.sender_member) : undefined} />
                    </div>;
                })}
                {running.map(member => <TypingRow key={`typing-${member.id}`} member={member} />)}
                {members.map(member => <MemberQuestion key={`question-${member.id}`} member={member} />)}
            </div>
        </div>

        <div className="flex-none px-4 pb-4 pt-2" data-aipp-slot="butler-group-input">
            <div className="mx-auto max-w-4xl">
                {quote && <div className="mb-2 flex"><ButlerQuoteChip quote={quote} onClear={() => setQuote(null)} /></div>}
                <div className="relative flex items-end gap-2 rounded-2xl border border-border bg-background px-3 py-2 focus-within:border-ring">
                    {mention && options.length > 0 && <div role="listbox" aria-label="点名成员"
                        className="absolute bottom-full left-2 mb-2 w-56 overflow-hidden rounded-lg border bg-popover p-1 shadow-md">
                        {options.map((option, index) => <button key={option.key} type="button" role="option" aria-selected={index === mentionIndex}
                            className={`flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm ${index === mentionIndex ? "bg-muted" : "hover:bg-muted"}`}
                            onMouseDown={event => { event.preventDefault(); chooseMention(option.label); }}>
                            {option.member ? <ButlerAvatar id={option.member.assistant_id} chief={option.member.is_chief} size={20} figure />
                                : <Users className="size-4 text-muted-foreground" />}
                            <span className="truncate">{option.label}</span>
                        </button>)}
                    </div>}
                    <textarea ref={inputRef} rows={1} value={draft} aria-label="群聊消息"
                        placeholder="发消息，输入 @ 可以点名"
                        className="max-h-[200px] min-h-[24px] flex-1 resize-none bg-transparent py-1 text-sm outline-none placeholder:text-muted-foreground"
                        onChange={event => { setDraft(event.target.value); updateMention(event.target.value, event.target.selectionStart ?? event.target.value.length); }}
                        onSelect={event => updateMention(event.currentTarget.value, event.currentTarget.selectionStart ?? 0)}
                        onBlur={() => setMention(null)}
                        onKeyDown={onKeyDown} />
                    {replying && <Button type="button" size="icon" variant="ghost" className="size-8 rounded-full" title="停止回复" aria-label="停止回复"
                        disabled={stopping} onClick={() => void stop()}>
                        {stopping ? <Loader2 className="size-4 animate-spin" /> : <Square className="size-3.5 fill-current" />}
                    </Button>}
                    <Button type="button" size="icon" className="size-8 rounded-full" title="发送" aria-label="发送"
                        disabled={!draft.trim() || sending} onClick={() => void send()}>
                        {sending ? <Loader2 className="size-4 animate-spin" /> : <ArrowUp className="size-4" />}
                    </Button>
                </div>
                {inputError && <p role="alert" className="mt-1.5 whitespace-pre-wrap text-xs text-destructive">{inputError}</p>}
            </div>
        </div>
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
