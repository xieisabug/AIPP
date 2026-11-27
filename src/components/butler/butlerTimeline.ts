import type { Message, MCPToolCallUpdateEvent } from "@/data/Conversation";
import { parseGroupTime } from "@/data/ButlerGroup";
import { parsePreviewFileRequestId } from "@/hooks/useInlineInteraction";
import { interactionContent, storedCallContent, type InteractionRow } from "./butlerInteractions";
import { isButlerInteractionTool } from "./ButlerPresentationContext";
import type { ProgressReport } from "./reportedProgress";
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
    watch_completion?: boolean | null;
}

export function withPeerTimeline(messages: Message[], unpublished: Set<number>, posts: PeerPost[], conversationId: number,
    interactions: InteractionRow[] = [], liveItems: InlineInteractionItem[] = [],
    calls?: ReadonlyMap<number, MCPToolCallUpdateEvent>, reports?: ReadonlyMap<number, ProgressReport[]>): Message[] {
    const questionIds = new Set(interactions.filter(row => row.kind === "question").map(row => row.request_id));
    const positions = new Map(messages.map((m, i) => [m.id, i]));
    const byRequest = new Map(liveItems.filter(item => item.requestId).map(item => [item.requestId, item]));
    const byCall = new Map(liveItems.filter(item => item.callId).map(item => [item.callId, item]));
    const ownerRows = new Map<number, Message[]>();
    const otherRows: Message[] = [];
    const positionedCalls = new Set<number>();
    const fileCalls = new Map<string, MCPToolCallUpdateEvent>();
    const conversationCalls = [...(calls?.values() ?? [])].filter(call => call.conversation_id === conversationId);
    const toolOwners = new Set(conversationCalls.flatMap(call => call.message_id ? [call.message_id] : []));
    const uiCalls = conversationCalls.filter(call => isButlerInteractionTool(call.tool_name));
    for (const call of uiCalls) {
        if (call.tool_name !== "preview_file") continue;
        const request = parsePreviewFileRequestId(call.result);
        if (request) fileCalls.set(request, call);
    }
    function addCard(id: number, anchor: number | null, time: number, content: Message["butler_interaction"], order: number, live?: InlineInteractionItem) {
        const index = anchor == null ? undefined : positions.get(anchor);
        const owner = index == null ? undefined : messages[index];
        const card: Message = {
            id, conversation_id: conversationId, parent_id: anchor, message_type: "butler_interaction", content: "",
            created_time: new Date(Number.isFinite(time) ? time : owner?.created_time ? parseGroupTime(owner.created_time).getTime() : 0),
            llm_model_id: null, start_time: null, finish_time: null, token_count: 0, input_token_count: 0, output_token_count: 0,
            regenerate: null, butler_interaction: content, butler_live_item_key: live?.key, butler_event_order: order,
        };
        if (owner && (owner.message_type === "response" || owner.message_type === "assistant")) {
            const list = ownerRows.get(owner.id) ?? [];
            list.push(card);
            ownerRows.set(owner.id, list);
        } else {
            otherRows.push(card);
        }
    }
    for (const row of interactions) {
        // 开工提示有独立的上报时间。即使所属回复属于内部协作，也要按该时间展示。
        if (row.kind === "report") {
            otherRows.push({
                id: -3_000_000_000_000 - row.id, conversation_id: conversationId, parent_id: row.anchor,
                message_type: "butler_report", content: row.request.text, created_time: new Date(row.occurred_ms),
                butler_report_request_id: row.request_id, butler_event_order: row.request.call_id ?? row.id,
                llm_model_id: null, start_time: null, finish_time: null, token_count: 0,
                input_token_count: 0, output_token_count: 0, regenerate: null,
            });
            continue;
        }
        const callId = row.kind === "question" ? row.request.call_id : fileCalls.get(row.request_id)?.call_id;
        const live = byRequest.get(row.request_id) ?? (callId ? byCall.get(callId) : undefined);
        const content = interactionContent(row, live, conversationId);
        if (!content) continue;
        if (callId) positionedCalls.add(callId);
        const kindIndex = row.kind === "file" ? 1 : row.kind === "task" ? 2 : row.kind === "code" ? 3 : 0;
        addCard(-4_000_000_000_000 - 4 * row.id - kindIndex, row.anchor, row.occurred_ms, content, callId ?? row.id, live);
    }
    for (const call of uiCalls.sort((a, b) => a.call_id - b.call_id)) {
        if (positionedCalls.has(call.call_id) || !call.message_id || !positions.has(call.message_id)) continue;
        const live = byCall.get(call.call_id);
        if (!live && call.status !== "success" && !((call.tool_name === "preview_code" || call.tool_name === "present_choices") && (call.status === "executing" || (call.tool_name === "present_choices" && call.status === "failed")))) continue;
        positionedCalls.add(call.call_id);
        addCard(-6_000_000_000_000 - call.call_id, call.message_id, call.started_time ? parseGroupTime(call.started_time).getTime() : NaN,
            live?.content ?? storedCallContent(call), call.call_id, live);
    }
    const visible = butlerTranscript(messages, unpublished, questionIds, calls, reports, ownerRows, positionedCalls, toolOwners);
    // Replies become public when they finish. Tools and peer posts can occur anywhere between
    // the start and finish of the same native response. Use its position only to break ties.
    const entries: Array<{ time: number; position: number; message?: Message; post?: PeerPost }> = [];
    for (const message of [...visible, ...otherRows]) {
        const event = message.message_type === "butler_report" || message.message_type === "butler_interaction";
        const anchor = positions.get(event ? message.parent_id! : message.id);
        const owner = anchor == null ? undefined : messages[anchor];
        const reply = message.message_type === "response" || message.message_type === "assistant";
        const stamp = (reply && message.finish_time) || message.created_time;
        const time = stamp ? parseGroupTime(stamp).getTime() : NaN;
        const start = owner?.created_time ? parseGroupTime(owner.created_time).getTime() : NaN;
        entries.push({ time: Math.max(time || 0, start || 0),
            position: (anchor ?? messages.length) + (event ? (owner?.message_type === "user" ? 0.5 : -0.5) : 0), message });
    }
    for (const post of posts) {
        const anchor = positions.get(post.anchor);
        entries.push({ time: parseGroupTime(post.occurred_at).getTime(), position: (anchor ?? messages.length) + 0.25, post });
    }
    entries.sort((a, b) => a.time - b.time || a.position - b.position
        || (a.message?.butler_event_order ?? a.post?.id ?? 0) - (b.message?.butler_event_order ?? b.post?.id ?? 0));
    const result: Message[] = [];
    for (const { message, post } of entries) {
        if (message) { result.push(message); continue; }
        if (!post) continue;
        const previous = result.at(-1)?.butler_posts;
        // 直接答复用户的事件单独成行，不并入“N 条消息往来”，以便一眼看出谁已经答复。
        const answersUser = (p: PeerPost) => p.direct || (p.task_reply && p.to_user);
        if (previous && !post.task_id && !answersUser(post) && previous.every(p => !answersUser(p)) && previous.every(p => !p.task_id && p.root === post.root) && Math.abs(parseGroupTime(post.occurred_at).getTime() - parseGroupTime(previous[previous.length - 1].occurred_at).getTime()) < 600_000 &&
            (previous.every(p => p.peer_conversation === post.peer_conversation) || (post.outgoing && previous.every(p => p.outgoing)))) {
            previous.push(post);
            continue;
        }
        result.push({
            // Pending user inputs use small negative queue IDs. Timeline rows have a separate display-only range.
            id: Number.MIN_SAFE_INTEGER + post.id, conversation_id: conversationId,
            message_type: "butler_exchange", content: "", created_time: parseGroupTime(post.occurred_at),
            llm_model_id: null, start_time: null, finish_time: null, token_count: 0, input_token_count: 0, output_token_count: 0,
            regenerate: null, butler_posts: [post],
        });
    }
    return result;
}


// Presentation-only row. Never inserted into messages or sent to the backend.
export function withButlerProcessing(messages: Message[], conversationId: number, running: boolean, publishedReports?: Message["butler_published_reports"]): Message[] {
    if (!running) return messages;
    return [...messages, {
        id: Number.MIN_SAFE_INTEGER, conversation_id: conversationId,
        message_type: "butler_processing", content: "", created_time: new Date(), butler_published_reports: publishedReports,
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
