import { describe, expect, it } from "vitest";
import { collapseUserMessage } from "./userMessageCollapse";

const attachment = `<fileattachment name="notes.txt">${"附件全文".repeat(200)}</fileattachment>`;

describe("collapseUserMessage", () => {
    it("should not collapse a short message just because its attachment is long", () => {
        expect(collapseUserMessage(`看看这个\n${attachment}`, 300)).toBeNull();
    });

    it("should cut only visible text and keep attachment tags whole", () => {
        const text = "字".repeat(400);
        const collapsed = collapseUserMessage(`${text}\n${attachment}`, 300);
        expect(collapsed).toBe(`${"字".repeat(300)}…${attachment}`);
    });

    it("should count characters, not UTF-16 units", () => {
        expect(collapseUserMessage("😀".repeat(5), 5)).toBeNull();
        expect(collapseUserMessage("😀".repeat(6), 5)).toBe(`${"😀".repeat(5)}…`);
    });
});
