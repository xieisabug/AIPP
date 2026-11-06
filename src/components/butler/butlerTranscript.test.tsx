import { describe, expect, it } from "vitest";
import type { Message } from "@/data/Conversation";
import { butlerTranscript } from "./butlerTranscript";

const message = (id: number, message_type: string, content: string, extra: Partial<Message> = {}) =>
    ({ id, message_type, content, ...extra }) as Message;

describe("Butler transcript", () => {
    const step = message(3, "response", '正在读取。<!-- MCP_TOOL_CALL:{"tool_name":"read_file"} -->');
    const history = [message(1, "user", "查编号"), message(2, "reasoning", "找文件"), step,
        message(4, "tool_result", "原始输出"), message(5, "response", "编号 LT-7042")];

    it("should retain only the answer after a tool completes and after reloading history", () => {
        expect(butlerTranscript(history)).toEqual([history[0], history[4]]);
        expect(butlerTranscript(structuredClone(history))).toEqual([history[0], history[4]]);
        expect(step.content).toContain("MCP_TOOL_CALL");
    });

    it("should hide tool actions and technical errors without deleting history", () => {
        const error = message(6, "error", "读取报告失败：文件不存在");
        expect(butlerTranscript([...history, error], new Set([3]))).toEqual([history[0], history[4]]);
    });

    it("should hide streaming markers and internal callbacks without hiding a user quoting the same text", () => {
        const callback = message(4, "user", "系统任务回流", { metadata_json: '{"origin":"internal"}' });
        const quote = message(5, "user", callback.content);
        expect(butlerTranscript([message(2, "response", '<!-- MCP_TOOL_CALL_STREAMING:{"tool'), callback, quote])).toEqual([quote]);
    });

    it("should hide the whole internal turn and resume after a real user message", () => {
        const peer = message(2, "user", "请核对草稿", {metadata_json:'{"origin":"butler_peer","leg":"request"}'});
        const user = message(1, "user", "请核对草稿");
        const answer = message(3, "response", "复审通过");
        expect(butlerTranscript([user,peer,answer,user,answer])).toEqual([user,user,answer]);
    });

    it("should retain the current assistant's final summary after a peer reply, receipt or task callback", () => {
        for (const metadata of [{origin:"butler_peer",leg:"reply"},{origin:"butler_peer",leg:"receipt"},{origin:"internal"}]) {
            const callback = message(2,"user","内部回流",{metadata_json:JSON.stringify(metadata)});
            const summary = message(3,"response","审稿已完成，结论如下");
            expect(butlerTranscript([history[0],callback,summary])).toEqual([history[0],summary]);
        }
    });

    it("should never publish tool narration at any phase of a tool call", () => {
        for (const extra of [{}, {finish_time:null}, {tool_calls_json:'[{"id":"t1"}]'}]) {
            expect(butlerTranscript([history[0], {...step,...extra}])).toEqual([history[0]]);
        }
        const partial = message(6,"response","正在派发给助手");
        expect(butlerTranscript([history[0],partial],new Set([6]))).toEqual([history[0]]);
    });

    it("should hide preview tool cards too while preserving the final answer", () => {
        const preview = '<!-- MCP_TOOL_CALL:{"tool_name":"preview_code","call_id":8} -->';
        const delivered = message(3, "response", step.content + preview);
        expect(butlerTranscript([delivered, history[4]])).toEqual([history[4]]);
    });

    it("should hide silent replies from internal turns but keep a later summary", () => {
        const callback = message(2, "user", "系统任务提醒", { metadata_json: '{"origin":"internal"}' });
        const silent = message(3, "response", "  [[NO_REPLY]]  ");
        const summary = message(5, "response", "核查完成，结论如下");
        expect(butlerTranscript([history[0], callback, silent, callback, summary])).toEqual([history[0], summary]);
    });

    it("should drop interrupted retry fragments when rendering the final answer", () => {
        const partial = message(7, "response", "中断的半句话", { metadata_json: JSON.stringify({
            stream_retry: {session_id:"s1",attempt:1,status:"interrupted",error:"401"},
        })});
        expect(butlerTranscript([partial,history[4]])).toEqual([history[4]]);
    });

    it("should keep delivered attachments and an answer containing literal tool markup in a code example", () => {
        const attachment = message(2, "response", step.content, { attachment_list: [{ id: 1 }] });
        const example = message(5, "response", '示例\n```html\n<!-- MCP_TOOL_CALL:{"tool_name":"read_file"} -->\n```');
        expect(butlerTranscript([attachment, example]).map(m => m.id)).toEqual([2, 5]);
        expect(butlerTranscript([example])[0].content).toBe(example.content);
    });
});
