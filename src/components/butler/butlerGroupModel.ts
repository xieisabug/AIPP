import { parseGroupTime, type ButlerGroupMember, type ButlerGroupMessage } from "@/data/ButlerGroup";

/** 与后端 `EVERYONE_MENTIONS` 保持一致。 */
export const EVERYONE_MENTION = "所有人";

export interface MentionQuery {
    /** `@` 在文本中的位置。 */
    start: number;
    query: string;
}

/** 光标前最近的 `@xxx`（中间没有空白）就是正在输入的点名。 */
export function findMentionQuery(text: string, caret: number): MentionQuery | null {
    const before = text.slice(0, caret);
    const at = Math.max(before.lastIndexOf("@"), before.lastIndexOf("＠"));
    if (at < 0) return null;
    const previous = at > 0 ? before[at - 1] : "";
    if (/[A-Za-z0-9]/.test(previous)) return null;
    const query = before.slice(at + 1);
    if (/\s/.test(query) || query.length > 20) return null;
    return { start: at, query };
}

export interface MentionOption {
    key: string;
    label: string;
    member?: ButlerGroupMember;
}

export function mentionOptions(members: ButlerGroupMember[], query: string): MentionOption[] {
    const needle = query.trim().toLowerCase();
    const matched = members.filter(member => member.name.toLowerCase().includes(needle))
        .map(member => ({ key: `member-${member.id}`, label: member.name, member }));
    const everyone = members.length > 1 && EVERYONE_MENTION.includes(needle)
        ? [{ key: "everyone", label: EVERYONE_MENTION }] : [];
    return [...matched, ...everyone];
}

/** 用选中的名字替换正在输入的 `@xxx`，并在后面补一个空格。 */
export function applyMention(text: string, mention: MentionQuery, caret: number, label: string) {
    const inserted = `@${label} `;
    const next = text.slice(0, mention.start) + inserted + text.slice(caret);
    return { text: next, caret: mention.start + inserted.length };
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
