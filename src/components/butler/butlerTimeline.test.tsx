import { describe, expect, it } from "vitest";
import type { Message } from "@/data/Conversation";
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
        // 失败也是这一轮的结果，“正在处理…”不再停留
        expect(hasPublishedReply([row(1, "user"), row(2, "error")])).toBe(true);
    });
});
