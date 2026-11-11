import { parseGroupTime, type ButlerGroupMember, type ButlerGroupMessage } from "@/data/ButlerGroup";

/** 与后端 `EVERYONE_MENTIONS` 保持一致。 */
export const EVERYONE_MENTION = "所有人";
const EVERYONE_MENTION_ID = -1;

export interface MentionTarget {
    /** 成员 id；“所有人”用 -1，不会与成员冲突。 */
    id: number;
    name: string;
    description?: string;
    member?: ButlerGroupMember;
}

/**
 * 群聊输入框的 @ 候选：本群成员，多于一人时再加“所有人”。
 * 过滤、插入与键盘选择由通用输入框 InputArea 负责，和私聊的 @ 是同一套。
 */
export function mentionTargets(members: ButlerGroupMember[]): MentionTarget[] {
    const targets: MentionTarget[] = members.map(member => ({ id: member.id, name: member.name, member }));
    if (members.length > 1) targets.push({ id: EVERYONE_MENTION_ID, name: EVERYONE_MENTION, description: "群里每位成员都会回复" });
    return targets;
}

/** 合并向上翻页加载的旧消息与最新一页，按序号去重。 */
export function mergeMessages(older: ButlerGroupMessage[], latest: ButlerGroupMessage[]) {
    const bySeq = new Map<number, ButlerGroupMessage>();
    for (const message of [...older, ...latest]) bySeq.set(message.seq, message);
    return [...bySeq.values()].sort((a, b) => a.seq - b.seq);
}

const DIVIDER_GAP_MS = 5 * 60 * 1000;

/** 和聊天软件一样：第一条消息或与上一条相隔五分钟以上时显示时间。 */
export function showTimeDivider(message: ButlerGroupMessage, previous?: ButlerGroupMessage) {
    if (!previous) return true;
    return parseGroupTime(message.created_at).getTime() - parseGroupTime(previous.created_at).getTime() >= DIVIDER_GAP_MS;
}

export function formatDivider(value: string, now = new Date()) {
    const date = parseGroupTime(value);
    if (Number.isNaN(date.getTime())) return value;
    const time = date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
    const sameDay = date.toDateString() === now.toDateString();
    if (sameDay) return time;
    const yesterday = new Date(now);
    yesterday.setDate(now.getDate() - 1);
    if (date.toDateString() === yesterday.toDateString()) return `昨天 ${time}`;
    return `${date.toLocaleDateString(undefined, { month: "numeric", day: "numeric" })} ${time}`;
}

export function excerpt(text: string, limit = 60) {
    const flat = text.replace(/\s+/g, " ").trim();
    return flat.length > limit ? `${flat.slice(0, limit)}…` : flat;
}

/** 联系人列表里的群消息摘要。 */
export function groupPreview(message: ButlerGroupMessage | null) {
    if (!message) return "";
    if (message.kind !== "message") return excerpt(message.body, 40);
    const speaker = message.sender_member === null ? "我" : message.sender_name;
    return `${speaker}：${excerpt(message.body, 40)}`;
}

export const groupReadKey = (groupId: number) => `butler-group-read-${groupId}`;
