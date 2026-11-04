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

    it("should preserve a pending or failed action and visible error", () => {
        const error = message(6, "error", "读取报告失败：文件不存在");
        expect(butlerTranscript([...history, error], new Set([3]))).toEqual([history[0], step, history[4], error]);
    });

    it("should hide streaming markers and internal callbacks without hiding a user quoting the same text", () => {
        const callback = message(4, "user", "系统任务回流", { metadata_json: '{"origin":"internal"}' });
        const quote = message(5, "user", callback.content);
        expect(butlerTranscript([message(2, "response", '<!-- MCP_TOOL_CALL_STREAMING:{"tool'), callback, quote])).toEqual([quote]);
    });

    it("should keep a deliverable card without its sibling execution trace", () => {
        const preview = '<!-- MCP_TOOL_CALL:{"tool_name":"preview_code","call_id":8} -->';
        const delivered = message(3, "response", step.content + preview);
        const transcript = butlerTranscript([delivered, history[4]]);
        expect(transcript[0].content).toContain(preview);
        expect(transcript[0].content).not.toContain("read_file");
        expect(transcript[1]).toBe(history[4]);
    });

    it("should keep delivered attachments and an answer containing literal tool markup in a code example", () => {
        const attachment = message(2, "response", step.content, { attachment_list: [{ id: 1 }] });
        const example = message(5, "response", '示例\n```html\n<!-- MCP_TOOL_CALL:{"tool_name":"read_file"} -->\n```');
        expect(butlerTranscript([attachment, example]).map(m => m.id)).toEqual([2, 5]);
        expect(butlerTranscript([example])[0].content).toBe(example.content);
    });
});
