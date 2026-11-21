import type { SearchResultItem } from "@/components/chat-sidebar/types";
import type { Message } from "@/data/Conversation";

type ObjectValue = Record<string, unknown>;
const object = (v: unknown): ObjectValue => v && typeof v === "object" && !Array.isArray(v) ? v as ObjectValue : {};
const list = (v: unknown): unknown[] => Array.isArray(v) ? v : [];

/** 一次服务端工具调用，界面上画成与 search_web / fetch_url 相同的卡片。 */
export interface WebCall {
    /** 在本条回复内稳定：步骤序号-调用序号。 */
    key: string;
    type: "search" | "read" | "find";
    title: string;
    /** 悬停看到的完整内容（全部查询、完整网址）。 */
    detail: string;
    url?: string;
    /** 流式进行中：已开始、还没有结果。 */
    running: boolean;
    failed: boolean;
    /** 这次调用带回的来源，rank 为全条回复统一编号（与正文角标一致）。 */
    sources: SearchResultItem[];
}

/**
 * 回复结束后用完整的 server_tool_calls；流式进行中只有实时事件（call_started / call_completed），
 * 按「步骤-调用序号」合并成同样的调用列表，键与结束后一致，卡片不会重建。
 */
function webCallEntries(web: ObjectValue): {key: string; call: ObjectValue; running: boolean}[] {
    if (Array.isArray(web.server_tool_calls)) {
        return list(web.server_tool_calls).flatMap((step, stepIndex) =>
            list(object(step).calls).map((call, callIndex) => ({ key: `${stepIndex}-${callIndex}`, call: object(call), running: false })));
    }
    const calls = new Map<string, {call: ObjectValue; running: boolean}>();
    for (const entry of list(web.events)) {
        const event = object(entry);
        const call = object(event.call);
        if (typeof event.step !== "number" || typeof call.index !== "number") continue;
        const key = `${event.step}-${call.index}`;
        const known = calls.get(key);
        if (event.type === "call_started") {
            calls.set(key, { call: { ...call, ...known?.call }, running: known ? known.running : true });
        } else if (event.type === "call_completed") {
            calls.set(key, { call: { ...known?.call, ...call }, running: false });
        }
    }
    return [...calls].map(([key, value]) => ({ key, ...value }));
}

/** 侧栏里这次调用的来源条目 id：卡片点击定位与 useContextList 生成条目共用。 */
export const deepSeekWebContextId = (messageId: number, call: WebCall) => `deepseek-web-${messageId}-${call.key}`;

export interface WebReport {
    sources: SearchResultItem[];
    calls: WebCall[];
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
    const report: WebReport = { sources: [], calls: [], usedTools: false, citations: [], filtered: parsed.deepseek_filtered === true };
    const ranks = new Map<string, number>();
    function addSource(value: unknown): SearchResultItem | undefined {
        const { url, title, snippet } = object(value);
        if (typeof url !== "string" || !/^https?:\/\//i.test(url)) return;
        try { new URL(url); } catch { return; }
        const known = ranks.get(url);
        if (known) return report.sources[known - 1];
        const source: SearchResultItem = { url, rank: report.sources.length + 1, title: typeof title === "string" && title ? title : url,
            snippet: typeof snippet === "string" ? snippet : undefined };
        ranks.set(url, source.rank!);
        report.sources.push(source);
        return source;
    }
    for (const { key, call, running } of webCallEntries(web)) {
        report.usedTools = true;
        const action = object(call.action);
        const failed = call.status === "failed";
        const sources: SearchResultItem[] = [];
        const collect = (value: unknown) => {
            const source = addSource(value);
            if (source && !sources.includes(source)) sources.push(source);
        };
        const base = { key, running, failed, sources };
        if (action.type === "search") {
            const queries = list(action.queries).filter((q): q is string => typeof q === "string" && q.trim() !== "");
            report.calls.push({ ...base, type: "search", title: queries.join("、") || "网络搜索", detail: queries.join("\n") || "网络搜索" });
        } else if (action.type === "open_page" || action.type === "read_link") {
            const page = object(call.page);
            const url = typeof action.url === "string" ? action.url : typeof page.url === "string" ? page.url : "";
            const title = typeof page.title === "string" && page.title ? page.title : url || "阅读网页";
            report.calls.push({ ...base, type: "read", title, detail: url || title, url: url || undefined });
            if (!failed && !running) collect({ ...action, ...page });
        } else if (action.type === "find_in_page") {
            const pattern = typeof action.pattern === "string" ? action.pattern : "";
            report.calls.push({ ...base, type: "find", title: pattern || "页内查找", detail: pattern || "页内查找" });
        }
        list(call.sources).forEach(collect);
    }
    for (const annotation of list(web.annotations)) {
        const citation = object(object(annotation).url_citation);
        const rank = addSource(citation)?.rank;
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
