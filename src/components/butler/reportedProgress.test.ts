import { describe, expect, it } from "vitest";
import type { AgentActivityEvent, Message } from "@/data/Conversation";
import { reportedProgressByMessage } from "./reportedProgress";
import { withPeerTimeline } from "./butlerTimeline";

describe("saved Butler progress", () => {
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
        expect(render([reply], new Set([reply.id]))).toEqual([]);
        expect(render([{ ...reply, message_type: "user", metadata_json: '{"origin":"butler_peer","audience":"peer"}' }, reply])).toEqual([]);
    });
});
