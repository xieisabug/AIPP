/** Public group identities are distinct from assistant and execution conversation IDs. */
export interface ButlerGroup {
    id: number;
    name: string;
    goal: string;
    chief_member: number | null;
    seq: number;
    state: "active" | "stopped";
    seat_wait_seconds: number;
}

export interface ButlerGroupMember {
    id: number;
    group_id: number;
    assistant_id: number;
    name: string;
    conversation_id: number;
    joined_after: number;
    delivered_through: number;
    active: boolean;
}

export interface ButlerGroupMessage {
    group_id: number;
    seq: number;
    sender_member: number | null;
    sender_conversation?: number | null;
    sender_name: string;
    kind: "message" | "progress" | "result" | "question" | "notice";
    body: string;
    reply_to: number | null;
    reference_only: boolean;
    created_at: string;
}

export interface ButlerGroupInboxItem {
    id: number;
    group_id: number;
    member_id: number;
    conversation_id?: number;
    event_seq: number;
    reason: string;
    round_id: number | null;
    state: "queued" | "prepared" | "delivered" | "answered" | "waiting" | "failed" | "cancelled" | "uncertain";
    packet_id: number | null;
    error: string | null;
}

export interface ButlerGroupRound {
    id: number;
    group_id: number;
    event_seq: number;
    members: number[];
    next_slot: number;
    state: "active" | "completed" | "cancelled";
}

export interface ButlerGroupStopResult {
    member_id: number;
    conversation_id: number;
    succeeded: boolean;
    detail: string;
}

export interface ButlerGroupDetail {
    group: ButlerGroup;
    members: ButlerGroupMember[];
    inbox: ButlerGroupInboxItem[];
    rounds: ButlerGroupRound[];
    stop_reports: ButlerGroupStopResult[];
    tasks?: ButlerGroupTask[];
}

export interface ButlerGroupTask {
    conversation_id: number;
    member_id: number;
    title: string;
    parent_packet: number | null;
    state: string;
}

export interface ButlerGroupTaskRequest {
    request_key: string;
    member_id: number;
    title: string;
    prompt: string;
    source_seqs: number[];
}

export interface ButlerGroupPost {
    /** Keep the same key and payload for a network/UI retry. */
    request_key: string;
    body: string;
    reply_to: number | null;
    reference_only?: boolean;
    /** Membership epoch IDs, never assistant IDs inferred from names in text. */
    recipients: number[];
    /** Atomic topic + ordered seats; mutually exclusive with recipients. */
    ordered_members?: number[];
    independent_members?: number[];
}
