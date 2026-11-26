import { useGroupResults, positionGroupResults, type GroupQuestion } from "./butlerGroupResults";
import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { AskUserQuestionCard } from "@/components/InlineInteractionCards";
import type { InlineInteractionItem } from "@/components/ConversationUI";
import ConversationTurnRail, { makeTurnPreview, type TurnRailItem } from "@/components/conversation/ConversationTurnRail";
import VirtuosoMessageList from "@/components/conversation/VirtuosoMessageList";
import type { MessageScrollAlign } from "@/components/conversation/scrollHighlight";
import type { Message, StreamEvent } from "@/data/Conversation";
import type { ButlerGroupInteractionTurn, ButlerGroupMember, ButlerGroupMessage } from "@/data/ButlerGroup";
import { useAcpElicitation } from "@/hooks/useAcpElicitation";
import { useAskUserQuestion } from "@/hooks/useInlineInteraction";
import { useVaultUseRequests } from "@/hooks/useVaultUseRequests";
import { useVaultInputRequests } from "@/hooks/useVaultInputRequests";
import { VaultUseCard } from "@/components/conversation/VaultUseCard";
import { VaultInputCard } from "@/components/conversation/VaultInputCard";
import { useScrollManagement } from "@/hooks/useScrollManagement";
import ButlerAvatar from "./ButlerAvatar";
import ButlerGroupMessageRow, { ButlerGroupSpeaker, toChatMessage } from "./ButlerGroupMessageRow";
import { ButlerQuoteButton, ButlerQuoteJumpContext, quoteExcerpt } from "./ButlerQuote";
import { HelperSummaryRow, LiveReportBubbles } from "./ButlerProgressRow";
import { useButlerProgress } from "./butlerProgress";
import { formatDivider, showTimeDivider } from "./butlerGroupModel";
import { groupVaultRequests } from "./groupVaultRequests";

// 群聊只展示成员的最终发言：流式、推理、工具调用、版本切换都留在成员各自的执行会话里，列表的这些输入恒为空。
const NO_STREAMS = new Map<number, StreamEvent>();
const NO_FLAGS = new Map<number, boolean>();
const NO_TOOL_STATES = new Map<number, unknown>();
const NO_GENERATIONS = new Map<string, unknown>();
const NO_VERSIONS = new Map<string, number>();
const noop = () => {};
const noGenerationControl = () => null;

/** 系统提示与错误单独标类型：既不算用户发言（不进轮次导航、不开启新一轮），也不算成员回复。 */
function toTranscriptMessage(groupId: number, message: ButlerGroupMessage, quoted?: ButlerGroupMessage): Message {
    const chat = toChatMessage(groupId, message, quoted);
    return message.kind === "message" ? chat : { ...chat, message_type: `butler_group_${message.kind}` };
}

function TypingRow({ member, shown }: { member: ButlerGroupMember; shown?: string[] }) {
    const progress = useButlerProgress(member.conversation_id);
    return <div role="status" aria-label={`${member.name} 正在输入`} data-aipp-slot="butler-group-typing">
        <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={member.name}>
            <ButlerAvatar id={member.assistant_id} chief={member.is_chief} size={20} figure animated state="thinking" />
        </ButlerGroupSpeaker>
        <LiveReportBubbles reports={progress?.reports} published={shown} />
        {progress?.helper ? <HelperSummaryRow summary={progress.helper} memberName={member.name} /> : <div className="butler-peer-bubble flex items-center gap-2 text-muted-foreground">
            <span className="butler-typing-dots" aria-hidden="true"><i /><i /><i /></span>
        </div>}
    </div>;
}

/** 成员在执行中向用户提的问题，显示在群聊里，答案回到这位成员的原执行。 */
const MemberAskListener = memo(function MemberAskListener({ conversationId, onChange }: {
    conversationId: number; onChange: (id: number, question: GroupQuestion | null) => void;
}) {
    const question = useAskUserQuestion({ conversationId });
    const { pendingRequest, isDialogOpen, callId, messageId, viewMode, completedAnswers, readOnly, handleSubmit, handleCancel } = question;
    useEffect(() => {
        if (!isDialogOpen || !pendingRequest) { onChange(conversationId, null); return; }
        onChange(conversationId, {
            key: pendingRequest.request_id, call: callId ?? undefined, owner: messageId ?? undefined,
            time: Date.now(), answered: viewMode === "summary",
            content: <AskUserQuestionCard request={pendingRequest} isOpen viewMode={viewMode} completedAnswers={completedAnswers}
                readOnly={readOnly} onSubmit={handleSubmit} onCancel={handleCancel} />,
        });
        return () => onChange(conversationId, null);
    }, [conversationId, onChange, pendingRequest, isDialogOpen, callId, messageId, viewMode, completedAnswers, readOnly, handleSubmit, handleCancel]);
    return null;
});

function MemberQuestion({ member, onShown }: { member: ButlerGroupMember; onShown: () => void }) {
    const elicitation = useAcpElicitation({ conversationId: member.conversation_id });
    const shown = !!elicitation.questionRequest || !!elicitation.decisionError;
    useEffect(() => {
        if (shown) onShown();
    }, [shown, onShown]);
    if (!shown) return null;
    return <div data-aipp-slot="butler-group-question">
        <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={`${member.name} 想问你`} />
        <div className="min-w-0 max-w-[min(65%,48rem)]">
            {elicitation.questionRequest && <AskUserQuestionCard request={elicitation.questionRequest} isOpen viewMode="questionnaire"
                errorMessage={elicitation.decisionError} onSubmit={elicitation.handleSubmit} onCancel={elicitation.handleCancel} />}
            {!elicitation.questionRequest && elicitation.decisionError &&
                <p role="alert" className="text-sm text-destructive">{elicitation.decisionError}</p>}
        </div>
    </div>;
}

interface Props {
    groupId: number;
    historyLoaded?: boolean;
    sourceSeq?: number;
    onSourceLocated?: () => void;
    messages: ButlerGroupMessage[];
    members: ButlerGroupMember[];
    running: ButlerGroupMember[];
    turns: ButlerGroupInteractionTurn[];
}

/**
 * 群聊记录。与私聊（ConversationUI）用同一套消息列表：虚拟化、打开时贴底并跟随晚到的布局、
 * 发送滑动、引用/轮次跳转与高亮、左侧轮次导航、最后一轮布局都来自共享列表；
 * 群聊特有的发言人、时间分隔、系统提示通过自定义行接入，列表的改进会同时作用于私聊和群聊。
 */
export default function ButlerGroupTranscript({ groupId, historyLoaded = false, messages, members, running, turns, sourceSeq, onSourceLocated }: Props) {
    const { messagesEndRef, scrollContainerRef, handleUserScrollIntent, syncScrollState, smartScroll } =
        useScrollManagement({ disableTailObservation: true });
    const [pendingScrollMessageId, setPendingScrollMessageId] = useState<number | null>(null);
    const [pendingScrollAlign, setPendingScrollAlign] = useState<MessageScrollAlign>("center");
    const [shiningMessageIds, setShiningMessageIds] = useState<Set<number>>(() => new Set());
    const transcriptKey = `butler-group-${groupId}`;

    const bySeq = useMemo(() => new Map(messages.map(message => [message.seq, message])), [messages]);
    const membersById = useMemo(() => new Map(members.map(member => [member.id, member])), [members]);
    const chatMessages = useMemo(
        () => messages.map(message => toTranscriptMessage(groupId, message, message.reply_to ? bySeq.get(message.reply_to) : undefined)),
        [bySeq, groupId, messages],
    );
    const groupMessageByChatId = useMemo(
        () => new Map(chatMessages.map((chat, index) => [chat.id, messages[index]])),
        [chatMessages, messages],
    );
    const chatIdBySeq = useMemo(
        () => new Map(chatMessages.map((chat, index) => [messages[index].seq, chat.id])),
        [chatMessages, messages],
    );
    const dividerSeqs = useMemo(
        () => new Set(messages.filter((message, index) => showTimeDivider(message, messages[index - 1])).map(message => message.seq)),
        [messages],
    );

    // Listen once for all member conversations; cards belong to stable group rows, not live tail.
    const vault = useVaultUseRequests(members.map(member => member.conversation_id));
    const vaultInput = useVaultInputRequests(members.map(member => member.conversation_id));
    const useCards = useMemo(() => groupVaultRequests(groupId, messages, vault.requests), [groupId, messages, vault.requests]);
    const inputCards = useMemo(() => groupVaultRequests(groupId, messages, vaultInput.requests), [groupId, messages, vaultInput.requests]);
    const requestMember = (conversationId: number | null) => members.find(member => member.conversation_id === conversationId);

    const speakers = useMemo(() => {
        const map = new Map(members.map(m => [m.conversation_id, m]));
        for (const message of messages) {
            if (message.source_conversation_id && message.sender_assistant && !map.has(message.source_conversation_id)) {
                map.set(message.source_conversation_id, { id: message.sender_member!, conversation_id: message.source_conversation_id,
                    assistant_id: message.sender_assistant, name: message.sender_name, is_chief: false });
            }
        }
        return map;
    }, [messages, members]);
    const results = useGroupResults([...speakers.keys()].sort((a, b) => a - b).join(","));
    const [questions, setQuestions] = useState<Map<number, GroupQuestion>>(() => new Map());
    const onQuestion = useCallback((id: number, question: GroupQuestion | null) => {
        setQuestions(previous => {
            if (!question && !previous.has(id)) return previous;
            const next = new Map(previous);
            if (question) next.set(id, question);
            else next.delete(id);
            return next;
        });
    }, []);
    const { before, tail, currentReports } = useMemo(() => positionGroupResults(results, turns, messages, questions), [results, turns, messages, questions]);
    const renderResult = useCallback((event: (typeof tail)[number]) => {
        const member = speakers.get(event.conversation);
        return member && <div key={event.key} className="mb-2" data-aipp-slot={event.liveQuestion ? "butler-group-question" : undefined}>
            <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={event.liveQuestion ? `${member.name} 想问你` : member.name} />
            {event.content}
        </div>;
    }, [speakers]);

    const renderMessageRow = useCallback((chat: Message) => {
        const message = groupMessageByChatId.get(chat.id);
        if (!message) return null;
        const time = dividerSeqs.has(message.seq)
            ? <div className="py-1 text-center text-[11px] text-muted-foreground">{formatDivider(message.created_at)}</div>
            : null;
        const events = before.get(message.seq)?.map(renderResult);
        if (message.kind !== "message") {
            return <div data-group-message-seq={message.seq}>
                {events}
                {time}
                <div className="flex justify-center" data-aipp-slot={`butler-group-${message.kind}`}>
                    <span className={`max-w-[80%] whitespace-pre-wrap break-words rounded-lg px-3 py-1 text-center text-xs ${message.kind === "error" ? "bg-destructive/5 text-destructive" : "text-muted-foreground"}`}>
                        {message.body}
                        {message.kind === "notice" && <ButlerQuoteButton target={{
                            kind: "message", id: message.seq, label: "群提示", excerpt: quoteExcerpt(message.body),
                        }} />}
                    </span>
                </div>
            </div>;
        }
        return <div data-group-message-seq={message.seq}>
            {events}
            {time}
            <ButlerGroupMessageRow groupId={groupId} message={message}
                quoted={message.reply_to ? bySeq.get(message.reply_to) : undefined}
                sender={message.sender_member ? membersById.get(message.sender_member) : undefined}
                highlighted={shiningMessageIds.has(chat.id)} />
            {(useCards.bySeq.get(message.seq) ?? []).map(request => {
                const member = requestMember(request.conversationId);
                return <div key={`vault-use-${request.requestId}`} className="min-w-0 max-w-[min(65%,48rem)]" data-aipp-slot="butler-group-vault" data-vault-request-id={request.requestId}>
                    {member && <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={`${member.name} 请求使用凭据`} />}
                    <VaultUseCard request={request} onRespond={vault.respond} />
                </div>;
            })}
            {(inputCards.bySeq.get(message.seq) ?? []).map(request => {
                const member = requestMember(request.conversationId);
                return <div key={`vault-input-${request.requestId}`} className="min-w-0 max-w-[min(65%,48rem)]" data-aipp-slot="butler-group-vault" data-vault-request-id={request.requestId}>
                    {member && <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={`${member.name} 请你填写凭据`} />}
                    <VaultInputCard request={request} onSubmit={vaultInput.submit} onDecline={vaultInput.decline} />
                </div>;
            })}
        </div>;
    }, [bySeq, dividerSeqs, groupId, groupMessageByChatId, membersById, shiningMessageIds, before, renderResult, useCards, inputCards, members, vault.respond, vaultInput.submit, vaultInput.decline]);

    // 停在底部时跟到底部（与私聊收到新消息一致），上翻阅读时 smartScroll 不打扰。
    const follow = useCallback(() => {
        requestAnimationFrame(() => smartScroll(false, "auto"));
    }, [smartScroll]);
    // 成员回复与“正在输入”变化时跟随；用户自己的发言由列表的发送滑动接管，这里不抢。
    const lastReplyId = useMemo(() => {
        for (let index = chatMessages.length - 1; index >= 0; index -= 1) {
            if (chatMessages[index].message_type !== "user") return chatMessages[index].id;
        }
        return null;
    }, [chatMessages]);
    const typingKey = running.map(member => member.id).join(",");
    useEffect(() => {
        follow();
    }, [follow, lastReplyId, typingKey]);

    const liveTail = useMemo<InlineInteractionItem[]>(() => [{
        key: "butler-group-live-tail",
        content: <>
            {tail.map(renderResult)}
            {running.map(member => <TypingRow key={`typing-${member.id}`} member={member} shown={currentReports.get(member.conversation_id)} />)}
            {members.map(member => <MemberQuestion key={`question-${member.id}`} member={member} onShown={follow} />)}
        </>,
    }], [follow, members, running, tail, renderResult, currentReports]);

    const railTurns = useMemo<TurnRailItem[]>(
        () => chatMessages.filter(message => message.message_type === "user")
            .map(message => ({ id: message.id, preview: makeTurnPreview(message.content) })),
        [chatMessages],
    );
    const selectTurn = useCallback((messageId: number) => {
        // 先停掉贴底跟随，避免刚跳过去就被拉回底部；轮次导航把这一轮的发言顶到视口顶部。
        handleUserScrollIntent();
        setPendingScrollAlign("start");
        setPendingScrollMessageId(messageId);
    }, [handleUserScrollIntent]);
    // 引用标签里存的是群消息序号，换成列表里的消息 id 后走与私聊相同的跳转 + 闪烁高亮。
    const jumpToQuoted = useCallback((seq: number) => {
        const messageId = chatIdBySeq.get(seq);
        if (messageId === undefined) return;
        setPendingScrollAlign("center");
        setPendingScrollMessageId(messageId);
    }, [chatIdBySeq]);

    useEffect(() => {
        if (historyLoaded && sourceSeq && chatIdBySeq.has(sourceSeq)) {
            jumpToQuoted(sourceSeq);
            onSourceLocated?.();
        }
    }, [historyLoaded, sourceSeq, chatIdBySeq, jumpToQuoted, onSourceLocated]);

    return <div className="relative flex min-h-0 flex-1 flex-col" data-aipp-slot="butler-group-transcript">
        {useCards.pending.length + inputCards.pending.length > 0 && <p role="status" className="px-6 py-2 text-sm text-muted-foreground" data-aipp-slot="butler-group-vault-pending">
            有 {useCards.pending.length + inputCards.pending.length} 个凭据请求等待对应群消息加载，请加载较早消息。
        </p>}
        {members.map(member => <MemberAskListener key={member.conversation_id} conversationId={member.conversation_id} onChange={onQuestion} />)}
        <ConversationTurnRail turns={railTurns} scrollContainerRef={scrollContainerRef} onSelect={selectTurn} />
        <div ref={scrollContainerRef} onWheelCapture={handleUserScrollIntent} onTouchMoveCapture={handleUserScrollIntent}
            className="conversation-scroll-transparent-track box-border flex h-full min-h-0 flex-1 flex-col gap-4 overflow-y-auto p-6"
            style={{ overflowAnchor: "none" }} aria-label="群聊记录" data-aipp-slot="butler-group-messages">
            <ButlerQuoteJumpContext.Provider value={jumpToQuoted}>
                <VirtuosoMessageList
                    conversationId={transcriptKey}
                    historyLoaded={historyLoaded}
                    transcriptKey={transcriptKey}
                    allDisplayMessages={chatMessages}
                    streamingMessages={NO_STREAMS}
                    shiningMessageIds={shiningMessageIds}
                    shiningMcpCallId={null}
                    reasoningExpandStates={NO_FLAGS}
                    mcpToolCallStates={NO_TOOL_STATES}
                    generationGroups={NO_GENERATIONS}
                    selectedVersions={NO_VERSIONS}
                    getGenerationGroupControl={noGenerationControl}
                    handleGenerationVersionChange={noop}
                    // 每一行都由 renderMessageRow 渲染（成员发言的代码在它自己的执行会话里运行），默认行的回调不会用到
                    onCodeRun={noop}
                    onMessageRegenerate={noop}
                    onMessageEdit={noop}
                    onMessageFork={noop}
                    onToggleReasoningExpand={noop}
                    inlineInteractionItems={liveTail}
                    mergeAssistantMessages={false}
                    renderMessageRow={renderMessageRow}
                    scrollContainerRef={scrollContainerRef}
                    pendingScrollMessageId={pendingScrollMessageId}
                    pendingScrollAlign={pendingScrollAlign}
                    clearPendingScrollMessageId={setPendingScrollMessageId}
                    setShiningMessageIds={setShiningMessageIds}
                    onScrollStateChange={syncScrollState}
                    smartScroll={smartScroll}
                />
            </ButlerQuoteJumpContext.Provider>
            <div ref={messagesEndRef} data-aipp-slot="chat-messages-end-anchor" />
        </div>
    </div>;
}
