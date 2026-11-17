import type { Message } from "@/data/Conversation";
import { parsePreviewFileRequestId } from "@/hooks/useInlineInteraction";
import { extractMcpToolCalls } from "@/hooks/useMcpToolCallProcessor";
import { interactionContent, type InteractionRow } from "./butlerInteractions";
import type { InlineInteractionItem } from "@/components/ConversationUI";
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

export function withPeerTimeline(messages: Message[], unpublished: Set<number>, posts: PeerPost[], conversationId: number,
    interactions: InteractionRow[] = [], liveItems: InlineInteractionItem[] = [],
    calls?: ReadonlyMap<number, { tool_name?: string; result?: string }>): Message[] {
    const questionIds = new Set(interactions.filter(row => row.kind === "question").map(row => row.request_id));
    const visible = butlerTranscript(messages, unpublished, questionIds, calls);
    if (!posts.length && !interactions.length) return visible;
    const positions = new Map(messages.map((m, i) => [m.id, i]));
    const entries: Array<{ position: number; message?: Message; post?: PeerPost; time?: number }> =
        visible.map(message => ({ position: positions.get(message.id)!, message }));
    const owners = new Map(entries.map(entry => [entry.message!.id, entry]));
    const live = new Map(liveItems.map(item => [item.requestId, item]));
    const fileCalls = new Map<string, number>();
    if (interactions.some(row => row.kind === "file")) {
        for (const [id, call] of calls ?? []) {
            if (call.tool_name !== "preview_file") continue;
            const requestId = parsePreviewFileRequestId(call.result);
            if (requestId) fileCalls.set(requestId, id);
        }
    }
    for (const row of interactions) {
        const content = interactionContent(row, live.get(row.request_id), conversationId);
        if (!content) continue;
        const owner = row.anchor == null ? undefined : owners.get(row.anchor);
        const callId = row.kind === "question" ? row.request.call_id : fileCalls.get(row.request_id);
        if (owner?.message && callId && extractMcpToolCalls(owner.message.content).some(marker => marker.data.call_id === callId)) {
            // Some native drivers also mirror the bridge call into MCP history. The marker
            // already renders its saved file except while the live hook owns this exact call.
            if (row.kind === "file" && live.get(row.request_id)?.callId !== callId) continue;
            const message = owner.message;
            owner.message = { ...message, butler_inline_interactions: [...(message.butler_inline_interactions ?? []),
                { key: row.request_id, callId, messageId: message.id, content }] };
            continue;
        }
        const anchor = row.anchor == null ? undefined : positions.get(row.anchor);
        const next = anchor === undefined ? messages.findIndex(m => new Date(m.created_time).getTime() > row.occurred_ms) : -1;
        // Native tools execute inside the response, before its published final text.
        const position = anchor === undefined ? (next < 0 ? messages.length : next) - 0.5
            : anchor + (messages[anchor].message_type === "user" ? 0.125 : -0.125);
        entries.push({position, time: row.occurred_ms, message: {
            id: -4_000_000_000_000 - 2 * row.id - Number(row.kind === "file"), conversation_id: conversationId,
            message_type: "butler_interaction", content: "", created_time: new Date(row.occurred_ms),
            llm_model_id: null, start_time: null, finish_time: null, token_count: 0, input_token_count: 0,
            output_token_count: 0, regenerate: null, butler_interaction: content,
        }});
    }
    for (const post of posts) {
        const anchor = positions.get(post.anchor);
        const time = new Date(post.occurred_at).getTime();
        // A historical branch or a tool-only response can be absent from the displayed list.
        const next = anchor === undefined ? messages.findIndex(m => new Date(m.created_time).getTime() > time) : -1;
        entries.push({ position: anchor === undefined ? (next < 0 ? messages.length : next) - 0.5 : anchor + 0.25, post });
    }
    entries.sort((a, b) => a.position - b.position || (a.time ?? Date.parse(a.post?.occurred_at ?? "")) - (b.time ?? Date.parse(b.post?.occurred_at ?? "")) || (a.post?.id ?? 0) - (b.post?.id ?? 0));
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

/** 最后一条用户消息之后是否已经发布了结果（简洁视图中可见的回复，或这一轮的失败提示）。 */
export function hasPublishedReply(messages: Message[]): boolean {
    for (let index = messages.length - 1; index >= 0; index--) {
        const type = messages[index].message_type;
        if (type === "user") return false;
        if (type === "response" || type === "assistant" || type === "error") return true;
    }
    return false;
}
