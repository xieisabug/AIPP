import { describe, expect, it } from "vitest";
import type { AgentActivityEvent, MCPToolCallUpdateEvent, Message } from "@/data/Conversation";
import { reportedProgressByMessage } from "./reportedProgress";
import { withPeerTimeline } from "./butlerTimeline";

describe("saved Butler progress", () => {
    it("should interleave separate reports and question cards when the same native message owns all three", () => {
        const start = new Date("2026-11-17T15:51:09Z");
        const user = { id: 10, message_type: "user", content: "开始", created_time: start } as Message;
        const owner = { ...user, id: 11, message_type: "response", content: "过程。最终答复", butler_final_text: "最终答复",
            finish_time: new Date(start.getTime() + 1000) } as Message;
        const call = (id: number, name: string): MCPToolCallUpdateEvent => ({
            conversation_id: 1548, message_id: 11, call_id: id, status: "success", tool_name: name,
            parameters: JSON.stringify({text:"进度"}), started_time: start,
        });
        const calls = new Map([call(12, "report_progress"), call(13, "ask_user_question"), call(14, "report_progress"),
            call(15, "read_file")].map(c => [c.call_id, c]));
        const reports = reportedProgressByMessage(1548, [], calls.values());
        const timeline = (pending: Set<number>) => withPeerTimeline([user, owner], pending, [], 1548, [],
            [{ key:"q", callId:13, content:"问题" }], calls, reports).map(row => row.butler_interaction ?? row.content);
        expect(timeline(new Set([11]))).toEqual(["开始","进度","问题","进度"]);
        expect(timeline(new Set())).toEqual(["开始","进度","问题","进度","最终答复"]);

        const native = { conversation_id: 1548, response_message_id: 11, kind: "tool", status: "success", order: 12,
            metadata: { tool:"aipp_t331_report_progress" }, input: { text:"进度" }, started_at:start.toISOString() } as AgentActivityEvent;
        expect(reportedProgressByMessage(1548, [native], calls.values()).get(11)).toHaveLength(2);
        const saved = [12, 14].map(id => ({ id, kind:"report" as const, anchor:11, request_id:String(id),
            occurred_ms:start.getTime()+id, request:{ text:"进度", call_id:id } }));
        for (const legacy of [[], [native]]) {
            const fallback = reportedProgressByMessage(1548, legacy, legacy.length ? calls.values() : [], saved);
            const recorded = withPeerTimeline([user, owner], new Set(), [], 1548, saved, [], undefined, fallback)
                .filter(row => row.message_type === "butler_report");
            expect(recorded.map(row => row.created_time.getTime())).toEqual(saved.map(r => r.occurred_ms));
        }
    });
    it("should restore a successful Pi report when the completed reply has no body", () => {
        const activity = { conversation_id: 1548, response_message_id: 26602, kind: "tool", status: "success",
            metadata: { tool: "aipp_t331_report_progress" }, input: { text: "我先查一下。" } } as AgentActivityEvent;
        const reports = reportedProgressByMessage(1548, [activity, { ...activity, conversation_id: 2 },
            { ...activity, status: "failed", input: { text: "失败" } }], []);
        const reply = { id: 26602, message_type: "response", content: "", finish_time: new Date() } as Message;
        const render = (messages: Message[], pending = new Set<number>()) =>
            withPeerTimeline(messages, pending, [], 1548, [], [], undefined, reports).map(row => row.content);
        expect(render([reply])).toEqual(["我先查一下。"]);
        expect(render([{ ...reply, content: "做完了。" }])).toEqual(["我先查一下。", "做完了。"]);
        expect(render([reply], new Set([reply.id]))).toEqual(["我先查一下。"]);
        expect(render([{ ...reply, message_type: "user", metadata_json: '{"origin":"butler_peer","audience":"peer"}' }, reply])).toEqual([]);
    });
});
