import { describe, expect, it } from "vitest";
import { readWebReport, webQuestionToResend, withWebCitations } from "./deepSeekWeb";
import type { Message } from "@/data/Conversation";

const page = { url: "https://example.org/page", title: "资料" };
const metadata = JSON.stringify({deepseek_web: {server_tool_calls: [{calls: [
    {action: {type: "read_link", url: page.url}, page, signature: "signed"},
    {action: {type: "search", queries: ["甲", "乙"]}, sources: [page]},
]}], annotations: [{url_citation: {...page, start_index: 0, end_index: 2}}]}});

describe("DeepSeek web tools", () => {
    it("should count queries and read links and deduplicate overlapping sources", () => {
        const report = readWebReport(metadata)!;
        expect([report.searchCount, report.pageCount, report.sources.length]).toEqual([2, 1, 1]);
        expect(JSON.stringify(report)).not.toContain("signed");
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
