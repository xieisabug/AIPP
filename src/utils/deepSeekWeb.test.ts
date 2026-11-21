import { describe, expect, it } from "vitest";
import { readWebReport, webQuestionToResend, withWebCitations } from "./deepSeekWeb";
import type { Message } from "@/data/Conversation";

const page = { url: "https://example.org/page", title: "资料" };
const other = { url: "https://example.org/other", title: "另一篇" };
const metadata = JSON.stringify({deepseek_web: {server_tool_calls: [
    {calls: [{action: {type: "read_link", url: page.url}, page, signature: "signed"}]},
    {calls: [
        {action: {type: "search", queries: ["甲", "乙"]}, sources: [page, other]},
        {action: {type: "open_page", url: "https://example.org/broken"}, status: "failed"},
        {action: {type: "find_in_page", pattern: "高度"}},
    ]},
], annotations: [{url_citation: {...page, start_index: 0, end_index: 2}}]}});

describe("DeepSeek web tools", () => {
    it("should turn each server call into a card entry with its own sources", () => {
        const report = readWebReport(metadata)!;
        expect(report.calls.map(c => [c.key, c.type, c.title, c.failed])).toEqual([
            ["0-0", "read", "资料", false],
            ["1-0", "search", "甲、乙", false],
            ["1-1", "read", "https://example.org/broken", true],
            ["1-2", "find", "高度", false],
        ]);
        expect(report.calls[1].detail).toBe("甲\n乙");
        expect(report.calls[1].sources.map(s => s.url)).toEqual([page.url, other.url]);
        expect(report.calls[2].sources).toEqual([]);
        expect(JSON.stringify(report)).not.toContain("signed");
    });
    it("should number sources once across calls so citations match the sidebar", () => {
        const report = readWebReport(metadata)!;
        expect(report.sources.map(s => [s.rank, s.url])).toEqual([[1, page.url], [2, other.url]]);
        expect(report.calls[1].sources.map(s => s.rank)).toEqual([1, 2]);
    });
    it("should build running calls from live events with the same keys as the final calls", () => {
        const live = readWebReport(JSON.stringify({deepseek_web: {events: [
            {type: "step_reasoning_done", step: 0},
            {type: "call_started", step: 1, call: {index: 0, action: {type: "search", queries: ["甲"]}}},
            {type: "call_started", step: 1, call: {index: 1, action: {type: "open_page", url: page.url}}},
            {type: "call_completed", step: 1, call: {index: 1, status: "completed", page}},
            {type: "call_completed", step: 1, call: {index: 2, status: "failed"}},
        ]}}))!;
        expect(live.calls.map(c => [c.key, c.type, c.running, c.failed])).toEqual([
            ["1-0", "search", true, false],
            ["1-1", "read", false, false],
        ]);
        expect(live.calls[1].sources.map(s => s.url)).toEqual([page.url]);
        // 结束后完整数据的同一调用沿用相同键；没有 status 的历史数据不算进行中
        expect(readWebReport(metadata)!.calls.every(c => !c.running)).toBe(true);
        expect(readWebReport(metadata)!.calls[1].key).toBe("1-0");
    });
    it("should place the citation after a supplementary Unicode character", () => {
        expect(withWebCitations("甲😀乙", readWebReport(metadata))).toBe("甲😀[⁽¹⁾](<https://example.org/page>)乙");
    });
    it("should resend the original question through a client tool continuation", () => {
        const message = (id: number, message_type: string, metadata_json?: string) =>
            ({id, conversation_id: 1, message_type, metadata_json}) as Message;
        const history = [message(4, "user"), message(5, "response", metadata), message(6, "tool_result"), message(7, "response")];
        expect(webQuestionToResend(history, 1)).toBe(4);
        expect(webQuestionToResend([...history, message(8, "user")], 1)).toBeUndefined();
        expect(webQuestionToResend(history, 2)).toBeUndefined();
    });
});
