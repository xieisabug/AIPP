import { describe, expect, it } from "vitest";
import type { Message, MCPToolCallUpdateEvent } from "@/data/Conversation";
import { hasPublishedReply, withButlerProcessing, withPeerTimeline, type PeerPost } from "./butlerTimeline";

const at = (minute: number) => new Date(Date.UTC(2026, 10, 4, 12, minute));
const raw = (id: number, type: string, content: string, metadata_json?: string) =>
    ({ id, message_type: type, created_time: at(id), content, metadata_json } as Message);
const post = (id: number, anchor: number, outgoing: boolean, peer: number, minute = anchor): PeerPost => ({
    id, anchor, outgoing, peer: String(peer), peer_conversation: peer, peer_assistant: peer,
    occurred_at: at(minute).toISOString(), text: String(id), status: "delivered", error: null,
});
const display = (messages: Message[], posts: PeerPost[]) =>
    withPeerTimeline(messages, new Set(), posts, 20).map(m => m.butler_posts?.map(p => p.id) ?? m.content);

describe("Butler peer timeline", () => {
    it("should append a transient processing row without changing stored history", () => {
        const messages = [raw(1,"user","请确认")];
        const active = withButlerProcessing(messages,20,true);
        expect(active.at(-1)?.message_type).toBe("butler_processing");
        expect(active.at(-1)?.conversation_id).toBe(20);
        expect(messages).toHaveLength(1);
        expect(withButlerProcessing(messages,20,false)).toBe(messages);
    });

    it("should show both recipients before the answer and keep the inbound envelope out of chat", () => {
        const messages = [raw(1,"user","请复核"), raw(2,"tool_result","内部输出"), raw(3,"response","已送审"),
            raw(4,"user","内部指令",'{"origin":"butler_peer"}'), raw(5,"response","已修订")];
        const posts = [post(1,2,true,30),post(2,2,true,40),post(3,4,false,30)];
        expect(display(messages, posts)).toEqual(["请复核",[1,2],"已送审",[3]]);
    });

    it("should show task results in chronological order before a human opens the contact", () => {
        const posts = [
            {...post(100,0,false,10,1),task_id:30},
            {...post(101,0,true,10,2),task_id:30,task_reply:true},
        ];
        expect(display([],posts)).toEqual([[100],[101]]);
        expect(display([raw(9,"user","谢谢")],posts)).toEqual([[100],[101],"谢谢"]);
    });

    it("should group adjacent rounds with one bot and split at a later user turn or time gap", () => {
        const messages = [raw(1,"user","初稿"),raw(4,"user","请确认")];
        const posts = [post(1,2,true,30),post(2,3,false,30),post(3,5,true,30),post(4,6,false,30,30)];
        expect(display(messages,posts)).toEqual(["初稿",[1,2],"请确认",[3],[4]]);
    });
});

describe("hasPublishedReply", () => {
    const row = (id: number, message_type: string) => ({ id, message_type } as Message);
    it("only counts replies after the latest user message", () => {
        expect(hasPublishedReply([row(1, "user"), row(2, "response")])).toBe(true);
        expect(hasPublishedReply([row(1, "response"), row(2, "user")])).toBe(false);
        expect(hasPublishedReply([row(1, "user"), row(2, "butler_exchange")])).toBe(false);
        // 失败也是这一轮的结果，处理中状态行不再停留
        expect(hasPublishedReply([row(1, "user"), row(2, "error")])).toBe(true);
    });
});

it("should keep the recovered answer at the original question across reload", () => {
    const question = { kind: "question" as const, id: 7, request_id: "q-7", anchor: null, occurred_ms: at(2).getTime(),
        request: { request_id: "q-7", questions: [] }, answers: { format: "PDF" } };
    const continuation = raw(4, "user", "[用户回答已恢复的问题，问题编号 q-7]\n问题：[]\n答案：{}\n旧权限不自动恢复。");
    const rows = [raw(1, "user", "生成报告"), raw(3, "response", "已收到"), continuation, raw(5, "response", "已交付")];
    for (const history of [rows, structuredClone(rows)]) {
        const timeline = withPeerTimeline(history, new Set(), [], 20, [question]);
        expect(timeline.map(row => row.message_type)).toEqual(["user", "butler_interaction", "response", "response"]);
    }
    expect(withPeerTimeline(rows, new Set(), [], 20)).toContain(continuation);
});

it("should keep an internal collaboration question between peer posts through answer and reload", () => {
    const messages = [raw(1, "user", "内部请求", '{"origin":"butler_peer","audience":"peer"}'),
        raw(2, "response", "内部回复")];
    const posts = [post(10, 1, false, 30, 1), post(11, 2, true, 30, 4)];
    const question = { kind: "question" as const, id: 7, request_id: "q-7", anchor: 2, occurred_ms: at(3).getTime(),
        request: { request_id: "q-7", questions: [] }, answers: null };
    const live = { key: "live-q-7", requestId: "q-7", messageId: 2, content: "待回答" };
    const answered = { ...question, answers: { format: "PDF" } };
    const pending = withPeerTimeline(messages, new Set([2]), posts, 20, [question], [live]);
    const submitted = withPeerTimeline(messages, new Set(), posts, 20, [answered], [live]);
    const reopened = withPeerTimeline(messages, new Set(), posts, 20, [answered]);

    for (const timeline of [pending, submitted, reopened]) {
        expect(timeline.map(row => row.butler_posts?.map(p => p.id) ?? row.message_type))
            .toEqual([[10], "butler_interaction", [11]]);
        expect(timeline[1].created_time).toEqual(at(3));
        expect(timeline[1].id).toBe(pending[1].id);
    }
    expect(pending[1].butler_interaction).toBe("待回答");
    expect(pending[1].butler_live_item_key).toBe(live.key);
    expect(submitted[1].butler_live_item_key).toBe(live.key);
    for (const timeline of [submitted, reopened]) {
        expect(timeline[1].butler_interaction).toMatchObject({ props: { viewMode: "summary", completedAnswers: answered.answers } });
    }
});

it("should position an MCP question from a hidden peer reply by its tool start time", () => {
    const messages = [raw(1, "user", "内部请求", '{"origin":"butler_peer","audience":"peer"}'), raw(2, "response", "内部回复")];
    const call: MCPToolCallUpdateEvent = { conversation_id: 20, message_id: 2, call_id: 42, tool_name: "ask_user_question",
        status: "success", started_time: at(3), finished_time: at(6) };
    const posts = [post(10, 1, false, 30, 1), post(11, 2, true, 30, 4)];
    const timeline = withPeerTimeline(messages, new Set(), posts, 20, [], [], new Map([[call.call_id, call]]));
    expect(timeline.map(row => row.butler_posts?.map(p => p.id) ?? row.message_type)).toEqual([[10], "butler_interaction", [11]]);
    expect(timeline[1].created_time).toEqual(at(3));
    expect(timeline[1].butler_interaction).toMatchObject({ props: { callId: 42 } });
});

it("should keep two native previews before their final reply in invocation order", () => {
    const files = [1, 2].map(id => ({ id, kind: "file" as const, anchor: 3, request_id: String(id),
        occurred_ms: id, request: '{"files":[]}' }));
    const live = files.map(row => ({ key: row.request_id, requestId: row.request_id, content: "file-" + row.id }));
    const rows = withPeerTimeline([raw(1, "user", "预览"), raw(3, "response", "完成")], new Set(), [], 20, files, live);
    expect(rows.map(row => row.butler_interaction ?? row.content)).toEqual(["预览","file-1","file-2","完成"]);
});
