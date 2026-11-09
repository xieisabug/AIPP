import { memo, useCallback, useMemo, type ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import MessageItem from "@/components/MessageItem";
import type { Message } from "@/data/Conversation";
import { isUserGroupMessage, parseGroupTime, type ButlerGroupMember, type ButlerGroupMessage } from "@/data/ButlerGroup";
import ButlerAvatar from "./ButlerAvatar";
import { ButlerQuoteButton, quoteExcerpt } from "./ButlerQuote";

export const groupSpeaker = (message: ButlerGroupMessage) => isUserGroupMessage(message) ? "我" : message.sender_name;

/** 群成员发言、正在输入、提问共用的发言人行：小头像 + 名字，放在内容上方，不占用左侧宽度。 */
export function ButlerGroupSpeaker({ assistantId, chief, label, children }: {
    assistantId: number;
    chief: boolean;
    label: string;
    children?: ReactNode;
}) {
    return <div className="mb-1.5 flex min-w-0 items-center gap-1.5 text-xs text-muted-foreground" data-aipp-slot="butler-group-speaker">
        {children ?? <ButlerAvatar id={assistantId} chief={chief} size={20} figure />}
        <span className="truncate">{label}</span>
    </div>;
}

/**
 * 把群消息转成私聊使用的消息结构，交给同一个 MessageItem 渲染（Markdown、代码预览、右键菜单、悬浮操作）。
 * 成员发言指向它执行会话里的那条最终回复，“消息信息”和代码运行都落在真实会话上；
 * 用户发言没有对应的会话消息，用负数序号作为本地标识。引用以私聊相同的引用标签显示。
 */
export function toChatMessage(groupId: number, message: ButlerGroupMessage, quoted?: ButlerGroupMessage): Message {
    const time = parseGroupTime(message.created_at);
    const fromUser = isUserGroupMessage(message);
    const quote = message.reply_to === null ? null
        : { label: quoted ? `${groupSpeaker(quoted)}：${quoteExcerpt(quoted.body, 40)}` : "引用的消息" };
    return {
        id: !fromUser && message.source_message_id ? message.source_message_id : -message.seq,
        conversation_id: message.source_conversation_id ?? groupId,
        message_type: fromUser ? "user" : message.source_message_id ? "response" : "assistant",
        content: message.body,
        llm_model_id: null,
        created_time: time,
        start_time: time,
        finish_time: time,
        token_count: 0,
        input_token_count: 0,
        output_token_count: 0,
        regenerate: null,
        attachment_list: [],
        metadata_json: quote ? JSON.stringify({ quote }) : null,
    };
}

interface Props {
    groupId: number;
    message: ButlerGroupMessage;
    quoted?: ButlerGroupMessage;
    sender?: ButlerGroupMember;
}

function ButlerGroupMessageRow({ groupId, message, quoted, sender }: Props) {
    const chatMessage = useMemo(() => toChatMessage(groupId, message, quoted), [groupId, message, quoted]);
    const quoteAction = useMemo(() => <ButlerQuoteButton variant="toolbar" target={{
        kind: "message", id: message.seq, label: groupSpeaker(message), excerpt: quoteExcerpt(message.body),
    }} />, [message]);
    // 和私聊一样运行/预览代码；成员发言在它自己的执行会话里运行，产物归属不变。
    const runCode = useCallback((lang: string, code: string) => {
        invoke("run_artifacts", { lang, inputStr: code, conversationId: message.source_conversation_id })
            .catch(error => toast.error(`运行失败：${String(error)}`));
    }, [message.source_conversation_id]);
    const item = <MessageItem message={chatMessage} conversationId={chatMessage.conversation_id}
        onCodeRun={runCode} messageActions={quoteAction} />;
    if (isUserGroupMessage(message)) {
        return <div data-aipp-slot="butler-group-user-message">{item}</div>;
    }
    // 与私聊同宽：气泡贴左边缘按整行宽度排版，只在上方多一行发言人（小头像 + 助手名）。
    return <div data-aipp-slot="butler-group-member-message">
        <ButlerGroupSpeaker assistantId={message.sender_assistant ?? 0} chief={sender?.is_chief ?? false} label={message.sender_name} />
        {item}
    </div>;
}

export default memo(ButlerGroupMessageRow);
