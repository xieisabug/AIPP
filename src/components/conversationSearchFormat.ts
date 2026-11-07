import type { ConversationSearchHit } from "../data/Conversation";

export const HIT_TYPE_LABELS: Record<ConversationSearchHit["hit_type"], string> = {
    title: "标题",
    summary: "摘要",
    message: "消息",
};

const MESSAGE_TYPE_LABELS: Record<string, string> = {
    user: "用户",
    response: "助手",
    assistant: "助手",
    reasoning: "思考",
    error: "错误",
    system: "系统",
};

export const speakerLabel = (hit: ConversationSearchHit) =>
    hit.speaker || (hit.message_type ? MESSAGE_TYPE_LABELS[hit.message_type] ?? hit.message_type : "");

/** 今天只显示时间，今年显示月日，更早显示完整日期 */
export const formatHitTime = (value: Date | string, now: Date = new Date()) => {
    const date = value instanceof Date ? value : new Date(value);
    if (Number.isNaN(date.getTime())) return "";
    const pad = (n: number) => String(n).padStart(2, "0");
    const time = `${pad(date.getHours())}:${pad(date.getMinutes())}`;
    if (date.toDateString() === now.toDateString()) return time;
    if (date.getFullYear() === now.getFullYear()) return `${date.getMonth() + 1}月${date.getDate()}日 ${time}`;
    return `${date.getFullYear()}/${date.getMonth() + 1}/${date.getDate()}`;
};
