import { describe, expect, it } from "vitest";
import { partialChoices, diffChoices } from "./presentChoices";
describe("候选渐进展示", () => {
    it("should preserve completed candidates and escaped partial text when a chunk ends mid-string", () => {
        const raw =
            '{"candidate_titles":["首版","次版"],"candidates":[{"id":"a","content":"括号 } 和引号 \\" 都是正文"},{"id":"b","content":"正在生成';
        const state = partialChoices(raw);
        expect(state.candidate_titles).toEqual(["首版", "次版"]);
        expect(state.candidates?.map((c) => c.content)).toEqual(['括号 } 和引号 " 都是正文', "正在生成"]);
    });
    it("should keep only complete unicode escapes when a chunk splits an escape", () => {
        expect(partialChoices('{"title":"ABC\\u4e2').title).toBe("ABC");
    });
    it("should mark the changed lines when two versions share a prefix and suffix", () => {
        expect(diffChoices("甲\n旧\n尾", "甲\n新\n尾").map((x) => x.kind)).toEqual([
            "same",
            "remove",
            "add",
            "same",
        ]);
    });
});

it.each([
    ["今天去公园散步。", "今天去海边散步。", "公园", "海边"],
    ["Use  the blue button!", "Use  the green button!", "blue", "green"],
    ["hello, world", "hello! world", ",", "!"],
])("should preserve surrounding words and whitespace when editing %s", (before, after, removed, added) => {
    const [oldLine, newLine] = diffChoices(before, after);
    expect(oldLine.parts?.filter(p => p.kind === "remove").map(p => p.text)).toEqual([removed]);
    expect(newLine.parts?.filter(p => p.kind === "add").map(p => p.text)).toEqual([added]);
    expect(oldLine.parts?.map(p => p.text).join("")).toBe(before);
    expect(newLine.parts?.map(p => p.text).join("")).toBe(after);
});
