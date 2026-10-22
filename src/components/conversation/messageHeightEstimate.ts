import type { Message } from "../../data/Conversation";
import { PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX } from "../../utils/previewCode";

// Pixel constants are calibrated against the rendered ChatUI (merged assistant
// bubbles, default font size). Virtuoso only consumes estimates before rows are
// measured, so each systematic error here becomes scrollbar movement while the
// user drags into unmeasured history.
export const DEFAULT_ESTIMATE_LIST_WIDTH_PX = 980;
const BUBBLE_MAX_WIDTH_RATIO = 0.65;
const BUBBLE_HORIZONTAL_PADDING_PX = 42;
const CJK_GLYPH_WIDTH_PX = 14;
const LATIN_GLYPH_WIDTH_PX = 7.6;
const CODE_GLYPH_WIDTH_PX = 7.8;

const TEXT_LINE_HEIGHT_PX = 24;
const BLOCK_GAP_PX = 14;
const HEADING_EXTRA_PX = 20;
const TABLE_ROW_HEIGHT_PX = 36;
const TABLE_CHROME_PX = 18;
const CODE_LINE_HEIGHT_PX = 20;
const CODE_BLOCK_CHROME_PX = 56;
const CODE_BLOCK_AUTO_COLLAPSE_LINE_THRESHOLD = 18;
const CODE_BLOCK_COLLAPSED_VISIBLE_LINE_COUNT = 16;
const HORIZONTAL_RULE_PX = 32;

const MCP_CARD_HEIGHT_PX = 54;
const AGENT_ACTIVITY_CARD_HEIGHT_PX = 88;
const MERGED_PART_GAP_PX = 8;
const BUBBLE_CHROME_PX = 34;
const STANDALONE_BUBBLE_CHROME_PX = 48;
const USER_BUBBLE_BASE_PX = 54;
const USER_LINE_HEIGHT_PX = 20;
const COLLAPSED_REASONING_HEIGHT_PX = 52;

const MCP_MARKUP_PATTERN = /<!--\s*MCP_TOOL_CALL(?:_STREAMING)?[\s\S]*?-->|<mcp_tool_call[\s\S]*?<\/mcp_tool_call>/gi;
const PREVIEW_CODE_PATTERN = /"tool_name"\s*:\s*"preview_code"|<tool_name>\s*preview_code\s*<\/tool_name>/i;
const INTERACTIVE_TOOL_PATTERN = /"tool_name"\s*:\s*"(ask_user_question|ask_user|request_user_input)"/;
const FENCE_PATTERN = /^\s{0,3}(`{3,}|~{3,})/;
const LIST_ITEM_PATTERN = /^\s*(?:[-*+]|\d{1,9}[.)])\s+/;
const HEADING_PATTERN = /^\s{0,3}#{1,6}\s/;
const TABLE_SEPARATOR_PATTERN = /^\s*\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$/;

export interface MessageHeightEstimateOptions {
    isLastMessage?: boolean;
    isReasoningExpanded?: boolean;
    agentActivityCount?: number;
    /** Rendered inside a merged assistant bubble (no own bubble chrome). */
    mergedMode?: boolean;
    isShowThinking?: boolean;
    listWidthPx?: number;
}

export function getBubbleContentWidthPx(listWidthPx?: number): number {
    const listWidth = listWidthPx && listWidthPx > 0 ? listWidthPx : DEFAULT_ESTIMATE_LIST_WIDTH_PX;
    return Math.max(160, listWidth * BUBBLE_MAX_WIDTH_RATIO - BUBBLE_HORIZONTAL_PADDING_PX);
}

function measureTextWidthPx(line: string, glyphWidthLatin: number): number {
    let width = 0;
    for (let index = 0; index < line.length; index += 1) {
        const code = line.charCodeAt(index);
        width += code > 0x2e7f ? CJK_GLYPH_WIDTH_PX : glyphWidthLatin;
    }
    return width;
}

function wrappedLineCount(line: string, contentWidthPx: number, glyphWidthLatin = LATIN_GLYPH_WIDTH_PX): number {
    const visible = line
        .replace(/\*\*|__|`/g, "")
        .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1");
    return Math.max(1, Math.ceil(measureTextWidthPx(visible, glyphWidthLatin) / contentWidthPx));
}

/** Height of rendered markdown text, without MCP cards. */
export function estimateMarkdownHeight(markdown: string, contentWidthPx: number): number {
    const lines = markdown.split(/\r?\n/);
    let height = 0;
    let blockCount = 0;
    let inParagraph = false;
    let tableRows = 0;
    let fence: { marker: string; lines: number } | null = null;

    const closeTable = () => {
        if (tableRows > 0) {
            height += TABLE_CHROME_PX + tableRows * TABLE_ROW_HEIGHT_PX;
            blockCount += 1;
            tableRows = 0;
        }
    };

    for (const rawLine of lines) {
        if (fence) {
            const closing = FENCE_PATTERN.exec(rawLine);
            if (closing && closing[1][0] === fence.marker[0] && closing[1].length >= fence.marker.length
                && rawLine.trim() === closing[1]) {
                const visualLines = fence.lines > CODE_BLOCK_AUTO_COLLAPSE_LINE_THRESHOLD
                    ? CODE_BLOCK_COLLAPSED_VISIBLE_LINE_COUNT
                    : Math.max(1, fence.lines);
                height += CODE_BLOCK_CHROME_PX + visualLines * CODE_LINE_HEIGHT_PX;
                blockCount += 1;
                fence = null;
            } else {
                fence.lines += wrappedLineCount(rawLine, contentWidthPx - 24, CODE_GLYPH_WIDTH_PX);
            }
            continue;
        }

        const line = rawLine.trimEnd();
        const fenceOpen = FENCE_PATTERN.exec(line);
        if (fenceOpen) {
            closeTable();
            inParagraph = false;
            fence = { marker: fenceOpen[1], lines: 0 };
            continue;
        }

        if (line.trim() === "") {
            closeTable();
            inParagraph = false;
            continue;
        }

        if (line.trimStart().startsWith("|")) {
            inParagraph = false;
            if (!TABLE_SEPARATOR_PATTERN.test(line)) {
                tableRows += 1;
            }
            continue;
        }
        closeTable();

        if (/^\s{0,3}(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)) {
            height += HORIZONTAL_RULE_PX;
            inParagraph = false;
            continue;
        }

        const isHeading = HEADING_PATTERN.test(line);
        const isListItem = LIST_ITEM_PATTERN.test(line);
        const indentPx = isListItem ? 24 : line.startsWith(">") ? 16 : 0;
        height += wrappedLineCount(line, contentWidthPx - indentPx) * TEXT_LINE_HEIGHT_PX;
        if (isHeading) {
            height += HEADING_EXTRA_PX;
        }
        if (isHeading || !inParagraph) {
            blockCount += 1;
        }
        inParagraph = !isHeading && !isListItem;
        if (isListItem) {
            height += 4;
        }
    }

    closeTable();
    if (fence) {
        const visualLines = Math.min(fence.lines, CODE_BLOCK_COLLAPSED_VISIBLE_LINE_COUNT);
        height += CODE_BLOCK_CHROME_PX + Math.max(1, visualLines) * CODE_LINE_HEIGHT_PX;
        blockCount += 1;
    }

    return height + Math.max(0, blockCount - 1) * BLOCK_GAP_PX;
}

function estimateToolCardsHeight(rawContent: string, isLastMessage: boolean): { height: number; count: number } {
    const segments = rawContent.match(MCP_MARKUP_PATTERN) ?? [];
    let height = 0;
    for (const segment of segments) {
        if (PREVIEW_CODE_PATTERN.test(segment) && !segment.includes("MCP_TOOL_CALL_STREAMING")) {
            height += PREVIEW_CODE_DEFAULT_VIEWPORT_HEIGHT_PX + (isLastMessage ? 32 : 8);
        } else if (INTERACTIVE_TOOL_PATTERN.test(segment)) {
            height += 120;
        } else {
            height += MCP_CARD_HEIGHT_PX;
        }
    }
    return { height, count: segments.length };
}

/**
 * Estimated rendered height of a single message element. In merged mode this
 * excludes the shared bubble chrome; otherwise it includes the message's own
 * bubble. Returns 0 for messages that render nothing.
 */
export function estimateMessageHeight(
    message: Pick<Message, "message_type" | "content" | "finish_time">,
    options: MessageHeightEstimateOptions = {},
): number {
    const {
        isLastMessage = false,
        isReasoningExpanded = false,
        agentActivityCount = 0,
        mergedMode = false,
        isShowThinking = true,
        listWidthPx,
    } = options;
    const rawContent = message.content ?? "";
    const contentWidthPx = getBubbleContentWidthPx(listWidthPx);
    const hasToolMarkup = rawContent.includes("MCP_TOOL_CALL") || /<mcp_tool_call/i.test(rawContent);

    if (message.message_type === "reasoning") {
        const isComplete = message.finish_time !== null;
        if (!isShowThinking) {
            return isComplete ? 0 : 36;
        }
        if (isComplete && !isReasoningExpanded) {
            if (!hasToolMarkup) {
                return COLLAPSED_REASONING_HEIGHT_PX;
            }
            return COLLAPSED_REASONING_HEIGHT_PX + estimateToolCardsHeight(rawContent, isLastMessage).height;
        }
        const text = rawContent.replace(MCP_MARKUP_PATTERN, "");
        return 48
            + estimateMarkdownHeight(text, contentWidthPx * 0.8)
            + estimateToolCardsHeight(rawContent, isLastMessage).height;
    }

    if (message.message_type === "user") {
        const text = rawContent.trim();
        const lineCount = text.split(/\r?\n/).reduce(
            (total, line) => total + wrappedLineCount(line, contentWidthPx),
            0,
        );
        return USER_BUBBLE_BASE_PX + Math.max(0, lineCount - 1) * USER_LINE_HEIGHT_PX;
    }

    const text = hasToolMarkup ? rawContent.replace(MCP_MARKUP_PATTERN, "\n\n").trim() : rawContent.trim();
    const textHeight = text ? estimateMarkdownHeight(text, contentWidthPx) : 0;
    const cards = hasToolMarkup ? estimateToolCardsHeight(rawContent, isLastMessage) : { height: 0, count: 0 };
    const activityHeight = agentActivityCount * AGENT_ACTIVITY_CARD_HEIGHT_PX;
    const partCount = (textHeight > 0 ? 1 : 0) + cards.count + agentActivityCount;
    const body = textHeight
        + cards.height
        + activityHeight
        + Math.max(0, partCount - 1) * BLOCK_GAP_PX;

    if (mergedMode) {
        return body;
    }
    return STANDALONE_BUBBLE_CHROME_PX + Math.max(TEXT_LINE_HEIGHT_PX, body);
}

/**
 * Height of one virtual row of a merged bubble that was split across rows.
 * The first segment carries the top chrome, the last the bottom chrome, and
 * every other segment ends with the inter-part gap, so the segments of a group
 * add up to {@link estimateMergedGroupHeight}.
 */
export function estimateMergedSegmentHeight(
    partHeights: readonly number[],
    { isFirst, isLast }: { isFirst: boolean; isLast: boolean },
): number {
    const visible = partHeights.filter((height) => height > 0);
    const halfChrome = BUBBLE_CHROME_PX / 2;
    return (isFirst ? halfChrome : 0)
        + (isLast ? halfChrome : MERGED_PART_GAP_PX)
        + visible.reduce((sum, height) => sum + height, 0)
        + Math.max(0, visible.length - 1) * MERGED_PART_GAP_PX;
}

/** Height of a merged assistant bubble containing the given part heights. */
export function estimateMergedGroupHeight(partHeights: readonly number[]): number {
    const visible = partHeights.filter((height) => height > 0);
    if (visible.length === 0) {
        return BUBBLE_CHROME_PX;
    }
    return BUBBLE_CHROME_PX
        + visible.reduce((sum, height) => sum + height, 0)
        + (visible.length - 1) * MERGED_PART_GAP_PX;
}
