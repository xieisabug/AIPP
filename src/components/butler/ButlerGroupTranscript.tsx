import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { MCPToolCall } from "@/data/MCPToolCall";
import { isButlerInteractionTool } from "./ButlerPresentationContext";
import ButlerInteractionResult from "./ButlerInteractionResult";
import { useCallback, useEffect, useMemo, useState } from "react";
import { AskUserQuestionCard } from "@/components/InlineInteractionCards";
import type { InlineInteractionItem } from "@/components/ConversationUI";
import ConversationTurnRail, { makeTurnPreview, type TurnRailItem } from "@/components/conversation/ConversationTurnRail";
import VirtuosoMessageList from "@/components/conversation/VirtuosoMessageList";
import type { MessageScrollAlign } from "@/components/conversation/scrollHighlight";
import type { Message, StreamEvent } from "@/data/Conversation";
import type { ButlerGroupMember, ButlerGroupMessage } from "@/data/ButlerGroup";
import { useAcpElicitation } from "@/hooks/useAcpElicitation";
import { useAskUserQuestion } from "@/hooks/useInlineInteraction";
import { useVaultUseRequests } from "@/hooks/useVaultUseRequests";
import { useVaultInputRequests } from "@/hooks/useVaultInputRequests";
import { VaultUseCard } from "@/components/conversation/VaultUseCard";
import { VaultInputCard } from "@/components/conversation/VaultInputCard";
import { useScrollManagement } from "@/hooks/useScrollManagement";
import ButlerAvatar from "./ButlerAvatar";
import ButlerGroupMessageRow, { ButlerGroupSpeaker, toChatMessage } from "./ButlerGroupMessageRow";
import { ButlerQuoteJumpContext } from "./ButlerQuote";
import { HelperSummaryRow } from "./ButlerProgressRow";
import { useButlerProgress } from "./butlerProgress";
import { formatDivider, showTimeDivider } from "./butlerGroupModel";

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

function TypingRow({ member }: { member: ButlerGroupMember }) {
    const progress = useButlerProgress(member.conversation_id);
    return <div role="status" aria-label={`${member.name} 正在输入`} data-aipp-slot="butler-group-typing">
        <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={member.name}>
            <ButlerAvatar id={member.assistant_id} chief={member.is_chief} size={20} figure animated state="thinking" />
        </ButlerGroupSpeaker>
        {progress?.reports.map(text => <div key={text} className="butler-peer-bubble break-words">{text}</div>)}
        {progress?.helper ? <HelperSummaryRow summary={progress.helper} /> : <div className="butler-peer-bubble flex items-center gap-2 text-muted-foreground">
            <span className="butler-typing-dots" aria-hidden="true"><i /><i /><i /></span>
        </div>}
    </div>;
}

/** 成员在执行中向用户提的问题，显示在群聊里，答案回到这位成员的原执行。 */
function MemberQuestion({ member, onShown }: { member: ButlerGroupMember; onShown: () => void }) {
    const question = useAskUserQuestion({ conversationId: member.conversation_id });
    const elicitation = useAcpElicitation({ conversationId: member.conversation_id });
    const vault = useVaultUseRequests(member.conversation_id);
    const vaultInput = useVaultInputRequests(member.conversation_id);
    // 已提交的问答由持久化结果卡接管，其余实时问题继续在这里回答。
    const showQuestion = question.isDialogOpen && question.viewMode !== "summary";
    const shown = showQuestion || !!elicitation.questionRequest || !!elicitation.decisionError
        || vault.requests.length > 0 || vaultInput.requests.length > 0;
    useEffect(() => {
        if (shown) onShown();
    }, [shown, onShown]);
    if (!shown) return null;
    return <div data-aipp-slot="butler-group-question">
        <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={`${member.name} 想问你`} />
        <div className="min-w-0 max-w-[min(65%,48rem)]">
            {showQuestion && <AskUserQuestionCard request={question.pendingRequest} isOpen={question.isDialogOpen}
                viewMode={question.viewMode} completedAnswers={question.completedAnswers} readOnly={question.readOnly}
                onSubmit={question.handleSubmit} onCancel={question.handleCancel} />}
            {elicitation.questionRequest && <AskUserQuestionCard request={elicitation.questionRequest} isOpen viewMode="questionnaire"
                errorMessage={elicitation.decisionError} onSubmit={elicitation.handleSubmit} onCancel={elicitation.handleCancel} />}
            {!elicitation.questionRequest && elicitation.decisionError &&
                <p role="alert" className="text-sm text-destructive">{elicitation.decisionError}</p>}
            {vault.requests.map(request => <VaultUseCard key={request.requestId} request={request} onRespond={vault.respond} />)}
            {vaultInput.requests.map(request => <VaultInputCard key={request.requestId} request={request}
                onSubmit={vaultInput.submit} onDecline={vaultInput.decline} />)}
        </div>
    </div>;
}

/**
 * 列表尾部的实时内容（与私聊的内联交互同一个位置）：正在输入的成员与成员提问。
 * 提问监听要一直挂着，问题来了才能出现；挂载时也会恢复尚未回答的问题。
 */
function GroupLiveTail({ members, running, onGrow }: { members: ButlerGroupMember[]; running: ButlerGroupMember[]; onGrow: () => void }) {
    return <>
        {running.map(member => <TypingRow key={`typing-${member.id}`} member={member} />)}
        {members.map(member => <MemberQuestion key={`question-${member.id}`} member={member} onShown={onGrow} />)}
    </>;
}

/** 群消息和成员执行会话分别存储。按源消息 ID 把预览放回该成员的回复旁，尚在执行的放在群聊末尾。 */
function useGroupResults(members: ButlerGroupMember[]) {
    const [calls, setCalls] = useState<Map<number, MCPToolCall[]>>(new Map());
    const memberConversations = members.map(member => member.conversation_id).join(",");
    useEffect(() => {
        let active = true;
        setCalls(new Map());
        const subscriptions = memberConversations.split(",").filter(Boolean).map(async id => {
            const conversationId = Number(id);
            let revision = 0;
            async function refresh() {
                const current = ++revision;
                try {
                    const rows = await invoke<MCPToolCall[]>("get_mcp_tool_calls_by_conversation", { conversationId });
                    if (!active || current !== revision) return;
                    const previews = rows.filter(call => call.conversation_id === conversationId
                        && isButlerInteractionTool(call.tool_name)
                        && (call.status === "success" || call.tool_name === "preview_code" && call.status === "executing"))
                        .sort((a, b) => a.id - b.id);
                    setCalls(previous => new Map(previous).set(conversationId, previews));
                } catch (error) {
                    console.error("读取群成员预览失败", conversationId, error);
                }
            }
            const off = await listen<{ type: string; data?: { tool_name?: string } }>(
                `conversation_event_${conversationId}`, ({ payload }) => {
                    if (payload.type === "mcp_tool_call_update" && (!payload.data?.tool_name || isButlerInteractionTool(payload.data.tool_name))) void refresh();
                });
            if (active) void refresh();
            return off;
        }).map(subscription => subscription.catch(error => {
            console.error("监听群成员交互结果失败", error);
        }));
        return () => {
            active = false;
            subscriptions.forEach(subscription => { void subscription.then(off => off?.()); });
        };
    }, [memberConversations]);
    return calls;
}

function groupPreview(call: MCPToolCall, live = false) {
    return <ButlerInteractionResult key={call.id} toolName={call.tool_name} callId={call.id} status={call.status}
        conversationId={call.conversation_id} messageId={call.assistant_message_id ?? call.message_id} parameters={call.parameters} isLastMessage={live} />;
}

interface Props {
    groupId: number;
    historyLoaded?: boolean;
    messages: ButlerGroupMessage[];
    members: ButlerGroupMember[];
    running: ButlerGroupMember[];
}

/**
 * 群聊记录。与私聊（ConversationUI）用同一套消息列表：虚拟化、打开时贴底并跟随晚到的布局、
 * 发送滑动、引用/轮次跳转与高亮、左侧轮次导航、最后一轮布局都来自共享列表；
 * 群聊特有的发言人、时间分隔、系统提示通过自定义行接入，列表的改进会同时作用于私聊和群聊。
 */
export default function ButlerGroupTranscript({ groupId, historyLoaded = false, messages, members, running }: Props) {
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

    const previewCalls = useGroupResults(members);
    const { previewsBySeq, pendingPreviews } = useMemo(() => {
        const previewsBySeq = new Map<number, MCPToolCall[]>();
        const pendingPreviews: MCPToolCall[] = [];
        for (const member of members) {
            const calls = previewCalls.get(member.conversation_id) ?? [];
            const replies = messages.filter(message => message.source_conversation_id === member.conversation_id && message.source_message_id);
            for (const call of calls) {
                const owner = call.assistant_message_id ?? call.message_id;
                const reply = replies.find(row => owner && row.source_message_id! >= owner);
                if (reply) {
                    const previews = previewsBySeq.get(reply.seq) ?? [];
                    previews.push(call);
                    previewsBySeq.set(reply.seq, previews);
                } else {
                    pendingPreviews.push(call);
                }
            }
        }
        return { previewsBySeq, pendingPreviews };
    }, [previewCalls, members, messages]);

    const renderMessageRow = useCallback((chat: Message) => {
        const message = groupMessageByChatId.get(chat.id);
        if (!message) return null;
        const time = dividerSeqs.has(message.seq)
            ? <div className="py-1 text-center text-[11px] text-muted-foreground">{formatDivider(message.created_at)}</div>
            : null;
        if (message.kind !== "message") {
            return <div>
                {time}
                <div className="flex justify-center" data-aipp-slot={`butler-group-${message.kind}`}>
                    <span className={`max-w-[80%] whitespace-pre-wrap break-words rounded-lg px-3 py-1 text-center text-xs ${message.kind === "error" ? "bg-destructive/5 text-destructive" : "text-muted-foreground"}`}>
                        {message.body}
                    </span>
                </div>
            </div>;
        }
        return <div>
            {time}
            <ButlerGroupMessageRow groupId={groupId} message={message}
                quoted={message.reply_to ? bySeq.get(message.reply_to) : undefined}
                sender={message.sender_member ? membersById.get(message.sender_member) : undefined}
                highlighted={shiningMessageIds.has(chat.id)} />
            {previewsBySeq.get(message.seq)?.map(call => groupPreview(call))}
        </div>;
    }, [bySeq, dividerSeqs, groupId, groupMessageByChatId, membersById, shiningMessageIds, previewsBySeq]);

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
            {pendingPreviews.map(call => {
                const member = members.find(item => item.conversation_id === call.conversation_id)!;
                return <div key={call.id}>
                    <ButlerGroupSpeaker assistantId={member.assistant_id} chief={member.is_chief} label={member.name} />
                    {groupPreview(call, true)}
                </div>;
            })}
            <GroupLiveTail members={members} running={running} onGrow={follow} />
        </>,
    }], [follow, members, running, pendingPreviews]);

    const turns = useMemo<TurnRailItem[]>(
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

    return <div className="relative flex min-h-0 flex-1 flex-col" data-aipp-slot="butler-group-transcript">
        <ConversationTurnRail turns={turns} scrollContainerRef={scrollContainerRef} onSelect={selectTurn} />
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
