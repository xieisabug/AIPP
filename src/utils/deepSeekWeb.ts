import type { SearchResultItem } from "@/components/chat-sidebar/types";
import type { Message } from "@/data/Conversation";

type ObjectValue = Record<string, unknown>;
const object = (v: unknown): ObjectValue => v && typeof v === "object" && !Array.isArray(v) ? v as ObjectValue : {};
const list = (v: unknown): unknown[] => Array.isArray(v) ? v : [];

export interface WebReport {
    sources: SearchResultItem[];
    steps: {type: "search" | "read" | "find"; label: string}[];
    searchCount: number;
    pageCount: number;
    usedTools: boolean;
    citations: {start: number; end: number; rank: number}[];
    filtered: boolean;
}

export function readWebReport(metadata?: string | null): WebReport | undefined {
    if (!metadata || !metadata.includes("deepseek_")) return;
    let parsed: ObjectValue;
    try { parsed = object(JSON.parse(metadata)); } catch { return; }
    if (!parsed.deepseek_web && !parsed.deepseek_filtered) return;
    const web = object(parsed.deepseek_web);
    const report: WebReport = { sources: [], steps: [], searchCount: 0, pageCount: 0, usedTools: false, citations: [], filtered: parsed.deepseek_filtered === true };
    const ranks = new Map<string, number>();
    function addSource(value: unknown) {
        const { url, title, snippet } = object(value);
        if (typeof url !== "string" || !/^https?:\/\//i.test(url)) return;
        try { new URL(url); } catch { return; }
        const known = ranks.get(url);
        if (known) return known;
        const rank = report.sources.length + 1;
        ranks.set(url, rank);
        report.sources.push({ url, rank, title: typeof title === "string" && title ? title : url,
            snippet: typeof snippet === "string" ? snippet : undefined });
        return rank;
    }
    for (const step of list(web.server_tool_calls)) {
        for (const entry of list(object(step).calls)) {
            const call = object(entry);
            report.usedTools = true;
            const action = object(call.action);
            const failed = call.status === 'failed';
            if (action.type === "search") {
                const queries = list(action.queries).filter((q): q is string => typeof q === "string");
                report.searchCount += Math.max(1, queries.length);
                report.steps.push({ type: "search", label: (queries.join("、") || "搜索") + (failed ? "（失败）" : "") });
            } else if (action.type === "open_page" || action.type === "read_link") {
                if (!failed) report.pageCount++;
                report.steps.push({ type: "read", label: String(action.url || "阅读网页") + (failed ? "（失败）" : "") });
                if (!failed) addSource({ ...action, ...object(call.page) });
            } else if (action.type === "find_in_page") {
                report.steps.push({ type: "find", label: String(action.pattern || "") });
            }
            list(call.sources).forEach(addSource);
        }
    }
    for (const annotation of list(web.annotations)) {
        const citation = object(object(annotation).url_citation);
        const rank = addSource(citation);
        const start = citation.start_index, end = citation.end_index;
        if (rank && typeof start === "number" && Number.isInteger(start) && start >= 0 &&
            typeof end === "number" && Number.isInteger(end) && end >= start) {
            report.citations.push({ start, end, rank });
        }
    }
    return report;
}

export function withWebCitations(content: string, report?: WebReport) {
    if (!report?.citations.length) return content;
    const offsets = [0];
    for (const ch of content) offsets.push(offsets[offsets.length - 1] + ch.length);
    const markers = new Map<number, Set<number>>();
    for (const {end, rank} of report.citations) {
        if (end >= offsets.length) continue;
        const at = offsets[end];
        const ranks = markers.get(at) ?? new Set();
        ranks.add(rank);
        markers.set(at, ranks);
    }
    for (const [at, ranks] of [...markers].sort(([a], [b]) => b - a)) {
        const mark = [...ranks].map(rank => {
            const url = new URL(report.sources[rank - 1].url).href.replace(/[()<>\\]/g, encodeURIComponentForMarkdown);
            const digits = Array.from(String(rank), d => "⁰¹²³⁴⁵⁶⁷⁸⁹"[Number(d)]).join("");
            return `[⁽${digits}⁾](<${url}>)`;
        }).join("");
        content = content.slice(0, at) + mark + content.slice(at);
    }
    return content;
}

function encodeURIComponentForMarkdown(char: string) {
    return "%" + char.charCodeAt(0).toString(16);
}

/** Find the original question even when the last message was produced after a client tool. */
export function webQuestionToResend(messages: Message[], conversation: number): number | undefined {
    let used = false;
    for (let i = messages.length - 1; i >= 0; i--) {
        const message = messages[i];
        if (message.conversation_id !== conversation) continue;
        if (message.message_type === "user") return used && message.id > 0 ? message.id : undefined;
        if (message.message_type === "response" && readWebReport(message.metadata_json)?.usedTools) used = true;
    }
}
