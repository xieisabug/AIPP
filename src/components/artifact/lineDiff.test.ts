import { describe, expect, it } from "vitest";
import { countChanges, diffLines } from "./lineDiff";

describe("diffLines", () => {
    it("should mark changed lines between shared head and tail", () => {
        const before = "a\nb\nc\nd\n";
        const after = "a\nB\nc\nd\ne\n";
        expect(diffLines(before, after)).toEqual([
            { kind: "same", text: "a" },
            { kind: "del", text: "b" },
            { kind: "add", text: "B" },
            { kind: "same", text: "c" },
            { kind: "same", text: "d" },
            { kind: "add", text: "e" },
        ]);
    });

    it("should align moved blocks by the longest common subsequence", () => {
        const lines = diffLines("x\n1\n2\n3\ny", "x\n2\n3\n4\ny");
        expect(lines.map((line) => `${line.kind[0]}${line.text}`)).toEqual(["sx", "d1", "s2", "s3", "a4", "sy"]);
        expect(countChanges(lines)).toEqual({ added: 1, removed: 1 });
    });

    it("should treat CRLF and LF the same and handle empty sides", () => {
        expect(diffLines("a\r\nb", "a\nb")).toEqual([
            { kind: "same", text: "a" },
            { kind: "same", text: "b" },
        ]);
        expect(diffLines("", "a")).toEqual([{ kind: "add", text: "a" }]);
        expect(diffLines("a", "")).toEqual([{ kind: "del", text: "a" }]);
    });

    it("should fall back to whole-block replacement for very large changes", () => {
        const before = Array.from({ length: 2100 }, (_, i) => `old${i}`).join("\n");
        const after = Array.from({ length: 2100 }, (_, i) => `new${i}`).join("\n");
        const { added, removed } = countChanges(diffLines(before, after));
        expect({ added, removed }).toEqual({ added: 2100, removed: 2100 });
    });
});
