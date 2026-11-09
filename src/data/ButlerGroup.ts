/** 群成员：成员 id 只在本群有效，与助手 id、执行会话 id 不同。 */
export interface ButlerGroupMember {
    id: number;
    assistant_id: number;
    name: string;
    /** 成员在本群的执行会话（问题卡片与审批按会话匹配）。 */
    conversation_id: number;
    is_chief: boolean;
}

export interface ButlerGroupMessage {
    group_id: number;
    seq: number;
    sender_member: number | null;
    sender_assistant: number | null;
    sender_name: string;
    /** message：用户或成员发言；notice / error：系统提示。 */
    kind: "message" | "notice" | "error";
    body: string;
    reply_to: number | null;
    created_at: string;
    /** 成员发言来自哪个执行会话的哪条回复；用户发言、系统提示和迁移前的旧消息为空。 */
    source_conversation_id: number | null;
    source_message_id: number | null;
}

export interface ButlerGroupSummary {
    id: number;
    name: string;
    members: ButlerGroupMember[];
    last_message: ButlerGroupMessage | null;
    updated_at: string;
    /** 正在回复的成员 id。 */
    typing: number[];
}

export interface ButlerGroupTurn {
    member_id: number;
    state: "queued" | "running";
    /** 多轮讨论的进度；普通回复为 1/1。 */
    round: number;
    rounds: number;
}

export interface ButlerGroupDetail {
    id: number;
    name: string;
    members: ButlerGroupMember[];
    messages: ButlerGroupMessage[];
    has_more: boolean;
    turns: ButlerGroupTurn[];
    /** 有消息正在等待判断由谁回复。 */
    routing: boolean;
}

export interface ButlerGroupStopSummary {
    stopped: number;
    errors: string[];
}

export const isUserGroupMessage = (message: ButlerGroupMessage) =>
    message.kind === "message" && message.sender_member === null;

/** 数据库时间是 UTC 的 `YYYY-MM-DD HH:MM:SS`。 */
export function parseGroupTime(value: string): Date {
    return new Date(/[zZ]|[+-]\d\d:?\d\d$/.test(value) ? value : `${value.replace(" ", "T")}Z`);
}
