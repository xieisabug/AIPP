/** 两版文本的逐行对比（「与上一版对比」）。 */

export type DiffLineKind = "same" | "add" | "del";

export interface DiffLine {
    kind: DiffLineKind;
    text: string;
}

/** 中段超过这个规模（行数乘积）就不再逐行对齐，直接显示「整段删除 + 整段新增」。 */
const MAX_LCS_CELLS = 4_000_000;

function splitLines(text: string): string[] {
    if (text === "") return [];
    const lines = text.replace(/\r\n/g, "\n").split("\n");
    if (lines[lines.length - 1] === "") lines.pop();
    return lines;
}

/** 先去掉相同的开头和结尾，再对中间部分做最长公共子序列。 */
export function diffLines(before: string, after: string): DiffLine[] {
    const a = splitLines(before);
    const b = splitLines(after);
    let start = 0;
    while (start < a.length && start < b.length && a[start] === b[start]) start++;
    let endA = a.length;
    let endB = b.length;
    while (endA > start && endB > start && a[endA - 1] === b[endB - 1]) {
        endA--;
        endB--;
    }
    const head: DiffLine[] = a.slice(0, start).map((text) => ({ kind: "same", text }));
    const tail: DiffLine[] = a.slice(endA).map((text) => ({ kind: "same", text }));
    const midA = a.slice(start, endA);
    const midB = b.slice(start, endB);
    return [...head, ...diffMiddle(midA, midB), ...tail];
}

function diffMiddle(a: string[], b: string[]): DiffLine[] {
    const n = a.length;
    const m = b.length;
    if (n === 0) return b.map((text) => ({ kind: "add", text }));
    if (m === 0) return a.map((text) => ({ kind: "del", text }));
    if (n * m > MAX_LCS_CELLS) {
        return [...a.map((text): DiffLine => ({ kind: "del", text })), ...b.map((text): DiffLine => ({ kind: "add", text }))];
    }
    // lcs[i][j]：a[i..] 与 b[j..] 的最长公共子序列长度
    const width = m + 1;
    const lcs = new Uint32Array((n + 1) * width);
    for (let i = n - 1; i >= 0; i--) {
        for (let j = m - 1; j >= 0; j--) {
            lcs[i * width + j] =
                a[i] === b[j] ? lcs[(i + 1) * width + j + 1] + 1 : Math.max(lcs[(i + 1) * width + j], lcs[i * width + j + 1]);
        }
    }
    const result: DiffLine[] = [];
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
        if (a[i] === b[j]) {
            result.push({ kind: "same", text: a[i] });
            i++;
            j++;
        } else if (lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]) {
            result.push({ kind: "del", text: a[i++] });
        } else {
            result.push({ kind: "add", text: b[j++] });
        }
    }
    while (i < n) result.push({ kind: "del", text: a[i++] });
    while (j < m) result.push({ kind: "add", text: b[j++] });
    return result;
}

export function countChanges(lines: DiffLine[]): { added: number; removed: number } {
    let added = 0;
    let removed = 0;
    for (const line of lines) {
        if (line.kind === "add") added++;
        else if (line.kind === "del") removed++;
    }
    return { added, removed };
}
