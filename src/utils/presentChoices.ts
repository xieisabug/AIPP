export type Candidate = {
    id: string;
    title: string;
    type: "text" | "markdown" | "html" | "image" | "artifact";
    summary?: string;
    content?: string;
    url?: string;
    entry_file?: string;
    language?: string;
};
export type ChoiceRequest = {
    title: string;
    prompt?: string;
    selection_mode: "single" | "multiple";
    layout: "tabs" | "split" | "grid";
    allow_feedback: boolean;
    candidates: Candidate[];
    candidate_titles?: string[];
    expected_count?: number;
};
export type ChoiceDecision = {
    status: "selected" | "cancelled" | "rejected";
    selected_ids: string[];
    feedback?: string;
};
export type ChoiceCard = {
    request_id: string;
    request: ChoiceRequest;
    decision?: ChoiceDecision;
    error?: string;
    recovered: boolean;
};

// Close only unfinished JSON containers and strings. A chunk may end in the middle of an escape,
// key, literal or number. The complete tool arguments are validated separately by the backend.
export function partialChoices(input: string): Partial<ChoiceRequest> & { completedCount?: number } {
    let i = 0,
        completedCount = 0;
    const space = () => {
        while (i < input.length && /\s/.test(input[i])) i++;
    };
    function string(): string {
        const start = i++;
        let last = i;
        while (i < input.length) {
            const ch = input[i++];
            if (ch === '"') return JSON.parse(input.slice(start, i));
            if (ch === "\\") {
                if (i >= input.length) break;
                if (input[i++] === "u") {
                    if (!/^[0-9a-f]{4}$/i.test(input.slice(i, i + 4))) break;
                    i += 4;
                }
            }
            last = i;
        }
        return JSON.parse(input.slice(start, last) + '"');
    }
    function value(depth: number, scope?: "candidates" | "candidate"): unknown {
        if (depth > 20) throw Error("嵌套过深");
        space();
        if (input[i] === '"') return string();
        if (input[i] === "{" || input[i] === "[") {
            const array = input[i++] === "[";
            const items: unknown[] = [];
            const fields: Record<string, unknown> = Object.create(null);
            space();
            while (i < input.length && input[i] !== (array ? "]" : "}")) {
                if (array) items.push(value(depth + 1, scope === "candidates" ? "candidate" : undefined));
                else {
                    if (input[i] !== '"') break;
                    const key = string();
                    space();
                    if (input[i++] !== ":") break;
                    fields[key] = value(depth + 1, depth === 0 && key === "candidates" ? "candidates" : undefined);
                }
                space();
                if (input[i] !== ",") break;
                i++;
                space();
            }
            if (input[i] === (array ? "]" : "}")) {
                i++;
                if (scope === "candidate" && !array) completedCount++;
            }
            return array ? items : fields;
        }
        const start = i;
        while (i < input.length && !/[,}\]\s]/.test(input[i])) i++;
        try {
            return JSON.parse(input.slice(start, i));
        } catch {
            return undefined;
        }
    }
    try {
        const raw = value(0) as Record<string, unknown>;
        const strings = (obj: Record<string, unknown>, keys: string[]) =>
            Object.fromEntries(keys.filter((key) => typeof obj[key] === "string").map((key) => [key, obj[key]]));
        const request: Partial<ChoiceRequest> = strings(raw, ["title", "prompt"]);
        if (Array.isArray(raw.candidate_titles))
            request.candidate_titles = raw.candidate_titles
                .slice(0, 6)
                .map((v) => (typeof v === "string" ? v : ""));
        if (Number.isInteger(raw.expected_count))
            request.expected_count = Math.min(6, Math.max(0, Number(raw.expected_count)));
        if (raw.layout === "tabs" || raw.layout === "grid" || raw.layout === "split") request.layout = raw.layout;
        if (Array.isArray(raw.candidates)) {
            request.candidates = raw.candidates.slice(0, 6).map((item, index) => {
                const c = item && typeof item === "object" ? item : {};
                const draft = strings(c, [
                    "id",
                    "title",
                    "type",
                    "content",
                    "url",
                    "summary",
                    "entry_file",
                    "language",
                ]);
                return { id: "draft-" + index, title: "", type: "text", ...draft } as Candidate;
            });
        }
        return { ...request, completedCount: Math.min(6, completedCount) };
    } catch {
        return {};
    }
}

type DiffKind = "same" | "add" | "remove";
type DiffPart = { kind: DiffKind; text: string };
type DiffRow = DiffPart & { parts?: DiffPart[] };

function diffSequence(a: string[], b: string[]): DiffPart[] {
    // Trim equal ends before allocating the LCS matrix, and bound memory for large manuscripts.
    let start = 0,
        end = 0;
    while (start < a.length && start < b.length && a[start] === b[start]) start++;
    while (a.length - end > start && b.length - end > start && a[a.length - end - 1] === b[b.length - end - 1])
        end++;
    const rows: DiffPart[] = a
        .slice(0, start)
        .map((text) => ({ kind: "same", text }));
    const x = a.slice(start, a.length - end),
        y = b.slice(start, b.length - end);
    const width = y.length + 1;
    if ((x.length + 1) * width > 1_000_000) {
        x.forEach((text) => rows.push({ kind: "remove", text }));
        y.forEach((text) => rows.push({ kind: "add", text }));
    } else {
        const lcs = new Uint32Array((x.length + 1) * width);
        for (let i = x.length - 1; i >= 0; i--)
            for (let j = y.length - 1; j >= 0; j--) {
                const k = i * width + j;
                lcs[k] = x[i] === y[j] ? 1 + lcs[k + width + 1] : Math.max(lcs[k + width], lcs[k + 1]);
            }
        let i = 0,
            j = 0;
        while (i < x.length || j < y.length) {
            if (i < x.length && j < y.length && x[i] === y[j]) {
                rows.push({ kind: "same", text: x[i++] });
                j++;
            } else if (j === y.length || (i < x.length && lcs[(i + 1) * width + j] >= lcs[i * width + j + 1]))
                rows.push({ kind: "remove", text: x[i++] });
            else rows.push({ kind: "add", text: y[j++] });
        }
    }
    if (end) a.slice(-end).forEach((text) => rows.push({ kind: "same", text }));
    return rows;
}

const wordSegmenter = typeof Intl.Segmenter === "function" ? new Intl.Segmenter("zh", { granularity: "word" }) : null;
function words(line: string): string[] {
    if (wordSegmenter) return Array.from(wordSegmenter.segment(line), (item) => item.segment);
    return line.match(/\p{Script=Han}|[\p{L}\p{N}\p{M}]+|\s+|./gu) || [];
}

function wordParts(tokens: DiffPart[], side: DiffKind) {
    const parts: DiffPart[] = [];
    for (const token of tokens) {
        if (token.kind !== "same" && token.kind !== side) continue;
        const previous = parts.at(-1);
        if (previous?.kind === token.kind) previous.text += token.text;
        else parts.push({ ...token });
    }
    return parts;
}

export function diffChoices(left: string, right: string): DiffRow[] {
    const rows: DiffRow[] = diffSequence(left.split("\n"), right.split("\n"));
    for (let i = 0; i < rows.length;) {
        if (rows[i].kind === "same") {
            i++;
            continue;
        }
        const removed: DiffRow[] = [];
        const added: DiffRow[] = [];
        while (i < rows.length && rows[i].kind !== "same") {
            const row = rows[i++];
            (row.kind === "add" ? added : removed).push(row);
        }
        for (let j = 0; j < Math.min(removed.length, added.length); j++) {
            const edits = diffSequence(words(removed[j].text), words(added[j].text));
            removed[j].parts = wordParts(edits, "remove");
            added[j].parts = wordParts(edits, "add");
        }
    }
    return rows;
}
