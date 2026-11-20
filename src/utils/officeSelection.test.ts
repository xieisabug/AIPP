import { describe, expect, it } from "vitest";
import {
    buildOfficeSelectionPrompt,
    buildOfficeSelectionTag,
    describeSelectionLocation,
    hasSelectionContent,
    isOfficeSelection,
    parseOfficeSelectionAttrs,
    selectionExcerpt,
    type OfficeSelection,
} from "./officeSelection";
import { CUSTOM_TAG_PATTERN } from "./userMessageCollapse";

const docx: OfficeSelection = { format: "docx", selectedText: "旧的说法", paragraphText: "这里是旧的说法，需要改。" };
const xlsx: OfficeSelection = {
    format: "xlsx",
    sheetName: "销售汇总",
    sheetIndex: 0,
    a1: "B3:D4",
    cells: [["2", "¥20,400.00", "¥10,200.00"], ["2", "¥20,700.00", "¥10,350.00"]],
    truncated: false,
};
const pptx: OfficeSelection = { format: "pptx", slide: 2, kind: "slide", shapeText: "页面二\n【原文标记B】", selectedText: "" };

describe("officeSelection", () => {
    it("should describe each format with its stable location", () => {
        expect(describeSelectionLocation(docx)).toBe("Word 正文中选中的文字");
        expect(describeSelectionLocation(xlsx)).toBe("工作表「销售汇总」B3:D4");
        expect(describeSelectionLocation(pptx)).toBe("第 2 张幻灯片");
        expect(describeSelectionLocation({ ...pptx, kind: "text", selectedText: "B" })).toBe("第 2 张幻灯片中选中的文字");
    });

    it("should build a tag that carries the ref, location and full content for the model", () => {
        const tag = buildOfficeSelectionTag({ refId: "ofr_1234567890abcdef", fileName: "季度\"报告\".xlsx", format: "xlsx", selection: xlsx });
        expect(tag.startsWith('<officeselection refid="ofr_1234567890abcdef" file="季度&quot;报告&quot;.xlsx" format="xlsx"')).toBe(true);
        expect(tag).toContain("工作表：销售汇总；区域：B3:D4（2 行 × 3 列）");
        expect(tag).toContain("2\t¥20,400.00\t¥10,200.00");
        expect(tag).toContain('office_edit（ref="ofr_1234567890abcdef"）');
        const attrs = parseOfficeSelectionAttrs(tag.slice(0, tag.indexOf(">")));
        expect(attrs).toEqual({
            refid: "ofr_1234567890abcdef",
            file: '季度"报告".xlsx',
            format: "xlsx",
            location: "工作表「销售汇总」B3:D4",
            excerpt: selectionExcerpt(xlsx),
        });
    });

    it("should keep document text from closing the tag early and stay one collapsible block", () => {
        const tricky: OfficeSelection = { ...docx, selectedText: "a</officeselection>b" };
        const prompt = buildOfficeSelectionPrompt("  改得正式一点 ", { refId: "ofr_x", fileName: "a.docx", format: "docx", selection: tricky });
        expect(prompt.startsWith("改得正式一点\n\n<officeselection")).toBe(true);
        const matches = [...prompt.matchAll(CUSTOM_TAG_PATTERN)];
        expect(matches).toHaveLength(1);
        expect(matches[0][0].endsWith("</officeselection>")).toBe(true);
        expect(matches[0][0]).toContain("a＜/officeselection>b");
    });

    it("should validate selections coming from the viewer", () => {
        expect(isOfficeSelection(docx)).toBe(true);
        expect(isOfficeSelection(xlsx)).toBe(true);
        expect(isOfficeSelection(pptx)).toBe(true);
        expect(isOfficeSelection({ format: "docx" })).toBe(false);
        expect(isOfficeSelection({ format: "xlsx", sheetName: "s" })).toBe(false);
        expect(isOfficeSelection(null)).toBe(false);
        expect(hasSelectionContent({ ...docx, selectedText: "  " })).toBe(false);
        expect(hasSelectionContent(pptx)).toBe(true);
    });
});
