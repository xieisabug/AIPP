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

    it("should show the delegate's own answer and question when the work was handed over to it", () => {
        const handed = message(2, "user", "[助手协作请求 · 来自 总管家]", {metadata_json:'{"origin":"butler_peer","leg":"request","audience":"user"}'});
        const answer = message(3, "response", "这是我写好的稿子");
        expect(butlerTranscript([handed, answer])).toEqual([answer]);
    });

    it("should keep internal help hidden, including the continuation after a nested reply", () => {
        const help = message(2, "user", "[助手协作请求 · 内部协作]", {metadata_json:'{"origin":"butler_peer","leg":"request","audience":"peer"}'});
        const material = message(3, "response", "给写作的资料");
        const nested = message(4, "user", "[助手协作答复]", {metadata_json:'{"origin":"butler_peer","leg":"reply","audience":"peer","parent":7}'});
        const forPeer = message(5, "response", "整理后回给写作");
        const user = message(6, "user", "用户的新问题");
        const reply = message(7, "response", "直接回答用户");
        expect(butlerTranscript([help, material, nested, forPeer, user, reply])).toEqual([user, reply]);
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

    it("should show a reply's leading quote marker as a quote of the earlier message it matches", () => {
        const review = message(1, "user", "明天的评审几点开始？");
        const budget = message(2, "user", "另外预算表谁来做？");
        const reply = message(3, "response", "[[QUOTE:评审几点开始]]\n下午两点，在三楼。");
        const [, , shown] = butlerTranscript([review, budget, reply]);
        expect(shown).toMatchObject({ id: 3, content: "下午两点，在三楼。", reply_quote: { label: "我：明天的评审几点开始？", target: 1 } });
        expect(reply.content).toContain("[[QUOTE:"); // 只改显示，历史记录本身不变
        // 引用自己之前的回复时不加“我：”
        const later = message(5, "response", "[[QUOTE:下午两点]]\n改到三点了。");
        expect(butlerTranscript([review, reply, message(4, "user", "确定吗"), later])[3].reply_quote)
            .toEqual({ label: "下午两点，在三楼。", target: 3 });
    });

    it("should drop a quote marker that matches no shown message instead of inventing a quote", () => {
        const internal = message(1, "user", "评审几点开始", { metadata_json: '{"origin":"internal"}' });
        const reply = message(2, "response", "[[QUOTE:评审几点开始]]\n下午两点。");
        const [shown] = butlerTranscript([internal, reply]);
        expect(shown.content).toBe("下午两点。");
        expect(shown.reply_quote).toBeUndefined();
    });
});
