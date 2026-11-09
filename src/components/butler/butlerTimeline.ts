import type { Message } from "@/data/Conversation";
import { butlerTranscript } from "./butlerTranscript";

export interface PeerPost {
    id: number;
    anchor: number;
    occurred_at: string;
    outgoing: boolean;
    peer: string;
    peer_conversation: number;
    peer_assistant: number | null;
    text: string;
    status: string;
    error: string | null;
    root?: number | null;
    task_id?: number | null;
    task_reply?: boolean;
    /** The executing assistant delivered directly to the user; show its answer in full under its own name. */
    direct?: boolean;
    /** The work was handed over for the receiver to answer the user itself (false: internal help between assistants). */
    to_user?: boolean;
    /** Parent exchange in a nested collaboration chain. */
    parent?: number | null;
    /** Speaker of a direct delivery when it differs from `peer` (e.g. a task result shown in the executor's own chat). */
    speaker?: string | null;
    speaker_assistant?: number | null;
    /** Exchange identity for quote replies; absent for task events. */
    exchange_id?: number | null;
}

export function withPeerTimeline(messages: Message[], unpublished: Set<number>, posts: PeerPost[], conversationId: number): Message[] {
    const visible = butlerTranscript(messages, unpublished);
    if (!posts.length) return visible;
    const positions = new Map(messages.map((m, i) => [m.id, i]));
    const entries: Array<{ position: number; message?: Message; post?: PeerPost }> =
        visible.map(message => ({ position: positions.get(message.id)!, message }));
    for (const post of posts) {
        const anchor = positions.get(post.anchor);
        const time = new Date(post.occurred_at).getTime();
        // A historical branch or a tool-only response can be absent from the displayed list.
        const next = anchor === undefined ? messages.findIndex(m => new Date(m.created_time).getTime() > time) : -1;
        entries.push({ position: anchor === undefined ? (next < 0 ? messages.length : next) - 0.5 : anchor + 0.25, post });
    }
    entries.sort((a, b) => a.position - b.position || Date.parse(a.post?.occurred_at ?? "") - Date.parse(b.post?.occurred_at ?? "") || (a.post?.id ?? 0) - (b.post?.id ?? 0));
    const result: Message[] = [];
    for (const { message, post } of entries) {
        if (message) { result.push(message); continue; }
        if (!post) continue;
        const previous = result.at(-1)?.butler_posts;
        // 直接答复用户的事件单独成行，不并入“N 条消息往来”，以便一眼看出谁已经答复。
        const answersUser = (p: PeerPost) => p.direct || (p.task_reply && p.to_user);
        if (previous && !post.task_id && !answersUser(post) && previous.every(p => !answersUser(p)) && previous.every(p => !p.task_id && p.root === post.root) && Math.abs(Date.parse(post.occurred_at) - Date.parse(previous[previous.length - 1].occurred_at)) < 600_000 &&
            (previous.every(p => p.peer_conversation === post.peer_conversation) || (post.outgoing && previous.every(p => p.outgoing)))) {
            previous.push(post);
            continue;
        }
        result.push({
            // Pending user inputs use small negative queue IDs. Timeline rows have a separate display-only range.
            id: Number.MIN_SAFE_INTEGER + post.id, conversation_id: conversationId,
            message_type: "butler_exchange", content: "", created_time: new Date(post.occurred_at),
            llm_model_id: null, start_time: null, finish_time: null, token_count: 0, input_token_count: 0, output_token_count: 0,
            regenerate: null, butler_posts: [post],
        });
    }
    return result;
}


// Presentation-only row. Never inserted into messages or sent to the backend.
export function withButlerProcessing(messages: Message[], conversationId: number, running: boolean): Message[] {
    if (!running) return messages;
    return [...messages, {
        id: Number.MIN_SAFE_INTEGER, conversation_id: conversationId,
        message_type: "butler_processing", content: "", created_time: new Date(),
        llm_model_id: null, start_time: null, finish_time: null, token_count: 0,
        input_token_count: 0, output_token_count: 0, regenerate: null,
    }];
}

/** 最后一条用户消息之后是否已经发布了助手回复（简洁视图中可见的 response/assistant）。 */
export function hasPublishedReply(messages: Message[]): boolean {
    for (let index = messages.length - 1; index >= 0; index--) {
        const type = messages[index].message_type;
        if (type === "user") return false;
        if (type === "response" || type === "assistant") return true;
    }
    return false;
}
