import { describe, expect, it } from "vitest";
import { splitTurnContext, visibleText, withTurnContext } from "./turnContext";

const block = [
    "<aipp_turn_context>",
    "以下是系统随这条消息附带的上下文，不是用户写的内容。",
    "<current_time>2026-11-10 17:00 (UTC+08:00) 周一</current_time>",
    "",
    "<long_term_memory full=\"true\" as_of=\"1794300992374\">",
    "- #2（已删除，不再适用）",
    "- #5 v2（已更新，替换之前的版本）（全局）：所有助手用中文",
    "- #3 v1：用户希望以后的回复中多用流程图来表示和说明内容。",
    "（另有 4 条记忆没有附带，需要时用 butler_memory 的 recall 按关键词查找）",
    "</long_term_memory>",
    "以上是长期记忆（#编号 v版本）。",
    "</aipp_turn_context>",
].join("\n");

describe("turnContext", () => {
    it("should split the user's text from the trailing context and join it back unchanged", () => {
        const text = "画一个流程图\n<fileattachment name=\"a.txt\">附件</fileattachment>";
        const stored = withTurnContext(text, block);
        const parts = splitTurnContext(stored);
        expect(parts.text).toBe(text);
        expect(parts.block).toBe(block);
        expect(parts.context?.startsWith("<current_time>")).toBe(true);
        expect(withTurnContext("改过的原话", parts.block)).toBe(`改过的原话\n\n${block}`);
    });

    it("should leave messages without a trailing context block untouched", () => {
        for (const content of ["普通消息", "怎么用 <aipp_turn_context> 这个标签？", `${block}\n后面还有话`]) {
            expect(splitTurnContext(content)).toEqual({ text: content, context: null, block: null });
            expect(visibleText(content)).toBe(content);
        }
        expect(withTurnContext("原话", null)).toBe("原话");
    });
});
