import { describe, expect, it } from "vitest";
import type { Message } from "@/data/Conversation";
import { butlerTranscript } from "./butlerTranscript";

const message = (id: number, message_type: string, content: string, extra: Partial<Message> = {}) =>
    ({ id, message_type, content, ...extra }) as Message;

describe("Butler transcript", () => {
    const step = message(3, "response", '正在读取。<!-- MCP_TOOL_CALL:{"tool_name":"read_file"} -->');
    const history = [message(1, "user", "查编号"), message(2, "reasoning", "找文件"), step,
        message(4, "tool_result", "原始输出"), message(5, "response", "编号 LT-7042")];

    it("should preserve the completed reply when a background wake starts and finishes", () => {
        const wake = message(6, "response", "截图已完成", {parent_id: 5, metadata_json: '{"origin":"native_background_wake"}'});
        expect(butlerTranscript([...history, {...wake, finish_time: null}])).toEqual([history[0], history[4]]);
        expect(butlerTranscript([...history, wake])).toEqual([history[0], history[4], wake]);
    });

    it("should publish only the final text when a turn has unmarked commentary or ends without a reply", () => {
        const process = message(2, "response", "先查一下");
        expect(butlerTranscript([history[0], process, history[4]])).toEqual([history[0], history[4]]);
        for (const terminal of [
            message(3, "response", "未完成", {finish_time:null}),
            message(3, "response", "[[NO_REPLY]]"),
            message(3, "response", "只有过程", {butler_final_text:""}),
            message(3, "response", "停止时的片段", {metadata_json:'{"end_reason":"cancelled"}'}),
        ]) expect(butlerTranscript([history[0], process, terminal])).toEqual([history[0]]);
        const response = message(3, "response", "执行记录和答复", {butler_final_text:"答复"});
        expect(butlerTranscript([history[0], process, response]).map(m => m.content)).toEqual(["查编号","答复"]);
        expect(response.content).toBe("执行记录和答复");
    });

    it("should retain only the answer after a tool completes and after reloading history", () => {
        expect(butlerTranscript(history)).toEqual([history[0], history[4]]);
        expect(butlerTranscript(structuredClone(history))).toEqual([history[0], history[4]]);
        expect(step.content).toContain("MCP_TOOL_CALL");
    });

    it("should show the failure of this turn as an error row and hide errors a later reply superseded", () => {
        const error = message(6, "error", "读取报告失败：文件不存在");
        expect(butlerTranscript([...history, error], new Set([3]))).toEqual([history[0], history[4], error]);
        const retried = message(6, "error", "读取报告失败：文件不存在", { isSupersededError: true });
        expect(butlerTranscript([history[0], retried, history[4]])).toEqual([history[0], history[4]]);
        // 仍在进行的这一轮不提前显示
        expect(butlerTranscript([history[0], error], new Set([6]))).toEqual([history[0]]);
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

    it("should show a scheduled task delivery even right after an internal turn", () => {
        const peer = message(2, "user", "[助手协作请求 · 内部协作]", {metadata_json:'{"origin":"butler_peer","leg":"request","audience":"peer"}'});
        const internalAnswer = message(3, "response", "给对方的素材");
        const trigger = message(4, "user", "[定时任务] 《周四休息提醒》", {metadata_json:'{"origin":"scheduled_task","role":"trigger","scheduled_task_id":6}'});
        const delivered = message(5, "response", "今天是休息日，记得拉伸。", {metadata_json:'{"origin":"scheduled_task","role":"result","scheduled_task_id":6,"scheduled_task_name":"周四休息提醒"}'});
        expect(butlerTranscript([peer, internalAnswer, trigger, delivered])).toEqual([delivered]);
        expect(butlerTranscript([peer, internalAnswer, delivered])).toEqual([delivered]);
    });

    it("should show persisted scheduled result prose around a tool block on delivery and reload", () => {
        const trigger = message(26312, "user", "[定时任务] 周日提醒", {
            metadata_json: '{"origin":"scheduled_task","role":"trigger"}',
        });
        const answer = "**今天周日休息。**";
        const tool = "<mcp_tool_call>工具参数</mcp_tool_call>";
        const delivered = message(26313, "response", answer + tool + "下周继续。", {
            parent_id: trigger.id, metadata_json: '{"origin":"scheduled_task","role":"result"}',
        });
        const rows = [trigger, delivered];
        for (const shown of [butlerTranscript(rows, new Set([delivered.id])), butlerTranscript(structuredClone(rows))]) {
            expect(shown.map(row => [row.id, row.content])).toEqual([[delivered.id, answer + "下周继续。"]]);
        }
        expect(butlerTranscript([{ ...delivered, metadata_json: null }])).toEqual([]);
        expect(delivered.content).toContain(tool);
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

    it.each(["preview_file", "preview_code", "ask_user_question"])("should show %s during execution and after reload", (name) => {
        const preview = `<!-- MCP_TOOL_CALL:{"tool_name":"${name}","call_id":8,"parameters":"<!-- example -->"} -->`;
        const delivered = message(3, "response", step.content + preview);
        expect(butlerTranscript([delivered], new Set([3]))[0].content).toBe(preview);
        expect(butlerTranscript([delivered, history[4]]).map(m => m.content)).toEqual([preview, history[4].content]);
    });

    it("should keep a preview_message card as a result but drop the narration around it", () => {
        const call = '<!-- MCP_TOOL_CALL:{"server_name":"Agent 工具","tool_name":"preview_message","parameters":"{\\"message_id\\":12}","call_id":9} -->';
        const shown = message(3, "response", `写作助手的初稿如下：${call}<!-- MCP_TOOL_CALL:{"tool_name":"read_file"} -->`);
        const [card, answer] = butlerTranscript([shown, history[4]]);
        expect(card.id).toBe(3);
        expect(card.content).toBe(call);
        expect(answer).toBe(history[4]);
        // 流式中的半截调用、内部回合与静默回复里的引用都不提前显示
        const streaming = message(3, "response", '<!-- MCP_TOOL_CALL_STREAMING:{"tool_name":"preview_message"');
        expect(butlerTranscript([streaming])).toEqual([]);
        expect(butlerTranscript([{ ...shown, finish_time: null } as Message])).toEqual([]);
        const internal = message(2, "user", "内部协作", { metadata_json: '{"origin":"butler_peer","leg":"request","audience":"peer"}' });
        expect(butlerTranscript([internal, shown])).toEqual([]);
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

    it("should preserve the previous completed reply while a sending optimistic user row has a negative id", () => {
        const sendingUser = message(-7_000_000_000_000, "user", "继续排查", {
            sendStatus: "sending",
            clientRequestId: "11111111-1111-4111-8111-111111111111",
        });
        const pendingResponse = message(6, "response", "", { finish_time: null });
        expect(butlerTranscript([...history, sendingUser, pendingResponse])).toEqual([
            history[0],
            history[4],
            sendingUser,
        ]);
    });
});
