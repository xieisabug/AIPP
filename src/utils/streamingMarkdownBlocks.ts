export interface ClosedMarkdownBlocks {
    closed: string[];
    open: string;
}

const FENCE_OPEN_PATTERN = /^\s*(`{3,}|~{3,})/;
const FENCE_CLOSE_PATTERN = /^\s*(`{3,}|~{3,})\s*$/;
const LIST_MARKER_PATTERN = /^(?:[-*+]|\d{1,9}[.)])(?:\s|$)/;
// HTML 块、自定义标签、引用式链接定义会跨块生效，切开后语义会变
const UNSPLITTABLE_LINE_PATTERN = /^\s{0,3}(?:<[A-Za-z!/?]|\[[^\]]+\]:\s)/;

function countMathDelimiters(line: string): number {
    return line.split("$$").length - 1;
}

/**
 * 在顶层块边界处切分：空行之后紧跟顶格、非列表项的行才切，保证每块单独解析与整体解析结果一致。
 * 已闭合的块可以单独 memo，流式 token 只重解析最后一块。
 * 内容里有无法安全切分的结构（HTML 块、链接定义、脚注）时返回 null，调用方应整体渲染。
 */
export function splitClosedMarkdownBlocks(content: string): ClosedMarkdownBlocks | null {
    if (!content) {
        return { closed: [], open: "" };
    }

    const closed: string[] = [];
    let current: string[] = [];
    let fence: { char: string; length: number } | null = null;
    let inMath = false;
    let sawBlank = false;

    const pushClosed = () => {
        const text = current.join("\n").replace(/^\n+|\n+$/g, "");
        current = [];
        if (text.trim()) {
            closed.push(text);
        }
    };

    for (const line of content.split("\n")) {
        if (fence) {
            current.push(line);
            const closeMatch = FENCE_CLOSE_PATTERN.exec(line);
            if (
                closeMatch
                && closeMatch[1][0] === fence.char
                && closeMatch[1].length >= fence.length
            ) {
                fence = null;
            }
            continue;
        }

        if (inMath) {
            current.push(line);
            if (countMathDelimiters(line) % 2 === 1) {
                inMath = false;
            }
            continue;
        }

        if (line.trim() === "") {
            sawBlank = current.length > 0;
            current.push(line);
            continue;
        }

        if (UNSPLITTABLE_LINE_PATTERN.test(line) || line.includes("[^")) {
            return null;
        }

        if (sawBlank && /^\S/.test(line) && !LIST_MARKER_PATTERN.test(line)) {
            pushClosed();
        }
        sawBlank = false;
        current.push(line);

        const fenceMatch = FENCE_OPEN_PATTERN.exec(line);
        if (fenceMatch) {
            fence = { char: fenceMatch[1][0], length: fenceMatch[1].length };
            continue;
        }
        if (countMathDelimiters(line) % 2 === 1) {
            inMath = true;
        }
    }

    return {
        closed,
        open: current.join("\n").replace(/^\n+/, ""),
    };
}
