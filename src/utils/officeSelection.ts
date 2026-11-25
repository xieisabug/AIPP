/**
 * Office 选区交给助手时附在用户消息里的 `<officeselection>` 块。
 * 模型读块里的完整内容（引用号、位置、原文）；聊天气泡只显示一个文件卡片（文件名、位置、摘录）。
 */
import type { ModernOfficeExtension } from "@/utils/officeFile";

export const MAX_SELECTION_ROWS = 50;
export const MAX_SELECTION_COLS = 20;

/** 与查看器插件 `protocol.ts` 的 OfficeSelection 一致；只含稳定定位，不含会话级 id。 */
export type OfficeSelection =
    | { format: "docx"; selectedText: string; paragraphText: string; paragraphOccurrence?: number; before?: string; after?: string }
    | {
          format: "xlsx";
          sheetName: string;
          sheetIndex: number;
          a1: string;
          cells: string[][];
          truncated: boolean;
      }
    | {
          format: "pptx";
          slide: number;
          kind: "slide" | "shape" | "text";
          shapeText: string;
          selectedText: string;
      };

export const OFFICE_SELECTION_TAG = "officeselection";

export function isOfficeSelection(value: unknown): value is OfficeSelection {
    if (!value || typeof value !== "object") return false;
    const v = value as Record<string, unknown>;
    switch (v.format) {
        case "docx":
            return typeof v.selectedText === "string" && typeof v.paragraphText === "string";
        case "xlsx":
            return typeof v.sheetName === "string" && typeof v.a1 === "string" && Array.isArray(v.cells);
        case "pptx":
            return typeof v.slide === "number" && typeof v.shapeText === "string";
        default:
            return false;
    }
}

function rangeSize(a1: string): { rows: number; cols: number } | null {
    const parse = (ref: string) => {
        const m = /^([A-Z]{1,3})(\d{1,7})$/i.exec(ref.trim());
        if (!m) return null;
        let col = 0;
        for (const ch of m[1].toUpperCase()) col = col * 26 + (ch.charCodeAt(0) - 64);
        return { row: Number(m[2]), col };
    };
    const [a, b] = a1.split(":");
    const start = parse(a);
    const end = b ? parse(b) : start;
    if (!start || !end) return null;
    return { rows: Math.abs(end.row - start.row) + 1, cols: Math.abs(end.col - start.col) + 1 };
}

/** 一行位置说明：给用户确认，也写进标签。 */
export function describeSelectionLocation(selection: OfficeSelection): string {
    switch (selection.format) {
        case "docx":
            return "Word 正文中选中的文字";
        case "xlsx":
            return `工作表「${selection.sheetName}」${selection.a1}`;
        case "pptx":
            return selection.kind === "text" && selection.selectedText
                ? `第 ${selection.slide} 张幻灯片中选中的文字`
                : selection.kind === "shape"
                  ? `第 ${selection.slide} 张幻灯片中选中的形状`
                  : `第 ${selection.slide} 张幻灯片`;
    }
}

function flat(text: string): string {
    return text.replace(/\s+/g, " ").trim();
}

/** 选区内容的简短摘录（界面展示用）。 */
export function selectionExcerpt(selection: OfficeSelection, max = 80): string {
    let text: string;
    switch (selection.format) {
        case "docx":
            text = selection.selectedText;
            break;
        case "xlsx":
            text = selection.cells.map((row) => row.join(" ")).join(" / ");
            break;
        case "pptx":
            text = selection.selectedText || selection.shapeText;
            break;
    }
    const f = flat(text);
    return f.length > max ? `${f.slice(0, max)}…` : f;
}

/** 选区是否有可交给助手的内容。 */
export function hasSelectionContent(selection: OfficeSelection | null): selection is OfficeSelection {
    if (!selection) return false;
    switch (selection.format) {
        case "docx":
            return selection.selectedText.trim().length > 0;
        case "xlsx":
            return selection.a1.length > 0;
        case "pptx":
            return selection.slide > 0;
    }
}

function escapeAttr(value: string): string {
    return value.replace(/&/g, "&amp;").replace(/"/g, "&quot;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function unescapeAttr(value: string): string {
    return value.replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&quot;/g, '"').replace(/&amp;/g, "&");
}

/** 块内文字不能提前闭合标签。 */
function safeBody(text: string): string {
    return text.replace(/<\/?officeselection/gi, (m) => m.replace("<", "＜"));
}

function selectionBody(selection: OfficeSelection): string {
    switch (selection.format) {
        case "docx":
            return [
                "选中的文字：",
                selection.selectedText,
                "",
                "所在段落全文（选区跨段时为第一段）：",
                selection.paragraphText,
                ...(selection.paragraphOccurrence ? [`相同段落中选中第 ${selection.paragraphOccurrence} 处，编辑时传 paragraphOccurrence=${selection.paragraphOccurrence} 和上述段落全文。`] : []),
                ...(selection.before ? [`选区前文：${selection.before}`] : []),
                ...(selection.after ? [`选区后文：${selection.after}`] : []),
            ].join("\n");
        case "xlsx": {
            const size = rangeSize(selection.a1);
            const lines = [
                `工作表：${selection.sheetName}；区域：${selection.a1}${size ? `（${size.rows} 行 × ${size.cols} 列）` : ""}`,
                "当前显示值（按行，单元格之间用制表符分隔）：",
                ...selection.cells.map((row) => row.join("\t")),
            ];
            if (selection.truncated) lines.push(`（区域较大，只附前 ${MAX_SELECTION_ROWS} 行 × ${MAX_SELECTION_COLS} 列）`);
            return lines.join("\n");
        }
        case "pptx": {
            const lines = [`第 ${selection.slide} 张幻灯片${selection.kind === "slide" ? "" : "中所选形状"}的文字：`, selection.shapeText || "（没有文字）"];
            if (selection.selectedText) lines.push("", "选中的文字：", selection.selectedText);
            return lines.join("\n");
        }
    }
}

export interface OfficeSelectionTagInput {
    refId: string;
    fileName: string;
    format: ModernOfficeExtension;
    selection: OfficeSelection;
}

type OfficeReviewFile = Omit<OfficeSelectionTagInput, "selection">;

function officeContextTag(file: OfficeReviewFile, location: string, excerpt: string, body: string): string {
    const attrs = [
        // ref 是 React 保留属性。
        `refid="${escapeAttr(file.refId)}"`,
        `file="${escapeAttr(file.fileName)}"`,
        `format="${file.format}"`,
        `location="${escapeAttr(location)}"`,
        `excerpt="${escapeAttr(excerpt)}"`,
    ].join(" ");
    return `<${OFFICE_SELECTION_TAG} ${attrs}>\n${safeBody(body)}\n</${OFFICE_SELECTION_TAG}>`;
}

export function buildOfficeSelectionTag(input: OfficeSelectionTagInput): string {
    const { selection } = input;
    const body = `${selectionBody(selection)}\n\n需要修改时调用 office_edit（ref="${input.refId}"），结果会另存为新文件，原文件不会被改动。`;
    return officeContextTag(input, describeSelectionLocation(selection), selectionExcerpt(selection), body);
}

/** 用户的修改要求 + 选区块，作为一条消息发送。 */
export function buildOfficeSelectionPrompt(request: string, tag: OfficeSelectionTagInput): string {
    return `${request.trim()}\n\n${buildOfficeSelectionTag(tag)}`;
}

export interface OfficeSuggestion {
    id: number;
    selection: OfficeSelection;
    comment: string;
}

export function buildOfficeReviewPrompt(
    ref: OfficeReviewFile & { documentText?: string | null },
    general: string,
    suggestions: OfficeSuggestion[],
): string {
    const blocks: string[] = [];
    const editInstruction = `请将本条消息中的修改意见合并为一次 office_edit（ref="${ref.refId}"）调用，生成一份副本。`;
    if (general.trim()) {
        if (!ref.documentText?.trim()) throw new Error("未取得文件正文，无法发送总意见");
        blocks.push(general.trim(), officeContextTag(ref, "整份文件", "", `${ref.documentText}\n\n${editInstruction}`));
    }
    for (const { id, selection, comment } of suggestions) {
        const body = blocks.length ? selectionBody(selection) : `${selectionBody(selection)}\n\n${editInstruction}`;
        blocks.push(`建议 ${id}：${comment}`, officeContextTag(ref, describeSelectionLocation(selection), selectionExcerpt(selection), body));
    }
    return blocks.join("\n\n");
}

export interface OfficeSelectionTagAttrs {
    refid: string;
    file: string;
    format: string;
    location: string;
    excerpt: string;
}

/** 从标签属性串（`refid="…" file="…"`）解析出属性。 */
export function parseOfficeSelectionAttrs(raw: string): OfficeSelectionTagAttrs {
    const get = (name: string) => {
        const m = new RegExp(`\\b${name}="([^"]*)"`).exec(raw);
        return m ? unescapeAttr(m[1]) : "";
    };
    return { refid: get("refid"), file: get("file"), format: get("format"), location: get("location"), excerpt: get("excerpt") };
}
