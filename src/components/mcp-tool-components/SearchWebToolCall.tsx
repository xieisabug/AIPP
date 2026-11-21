import React, { useMemo } from "react";
import { WebToolCard, formatDurationSeconds, parseToolParameters, useWebToolCall, type WebToolMeta } from "@/components/mcp-tool-components/WebToolCard";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";

interface ParsedSearchResult {
    engine?: string;
    resultCount?: number;
    searchTimeMs?: number;
}

const isRecord = (value: unknown): value is Record<string, unknown> => (
    Boolean(value) && typeof value === "object" && !Array.isArray(value)
);

const isToolContentPart = (value: unknown): value is Record<string, unknown> => (
    isRecord(value) && typeof value.type === "string" && ("json" in value || "text" in value)
);

const numberValue = (value: unknown): number | undefined => (
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : undefined
);

// Estimate result count from SERP markdown by counting distinct external links.
// SERP pages have many non-result links (nav, pagination, etc.), but result
// entries are usually the bulk of distinct external URLs — deduping gives a
// reasonable approximation. Returns undefined when no links are found.
const estimateResultCountFromMarkdown = (markdown: string): number | undefined => {
    if (!markdown) {
        return undefined;
    }
    const linkPattern = /\[[^\]]*\]\((https?:\/\/[^\s)]+)\)/g;
    const urls = new Set<string>();
    let match: RegExpExecArray | null;
    while ((match = linkPattern.exec(markdown)) !== null) {
        urls.add(match[1]);
    }
    return urls.size > 0 ? urls.size : undefined;
};

const applyStructuredSearchPayload = (payload: unknown, output: ParsedSearchResult): boolean => {
    if (Array.isArray(payload)) {
        output.resultCount = payload.length;
        return true;
    }
    if (!isRecord(payload)) {
        return false;
    }

    if (Array.isArray(payload.items)) {
        output.resultCount = payload.items.length;
    }

    const searchTimeMs = numberValue(payload.search_time_ms);
    if (typeof searchTimeMs === "number") {
        output.searchTimeMs = searchTimeMs;
    }

    if (typeof payload.search_engine === "string") {
        output.engine = payload.search_engine;
    }

    return Array.isArray(payload.items)
        || typeof payload.search_time_ms === "number"
        || typeof payload.search_engine === "string";
};

const getToolContentParts = (parsed: unknown): Record<string, unknown>[] => {
    if (Array.isArray(parsed)) {
        return parsed.filter(isToolContentPart);
    }
    if (isRecord(parsed) && Array.isArray(parsed.content)) {
        return parsed.content.filter(isToolContentPart);
    }
    return [];
};

const parseSearchResult = (result?: string, fallbackSearchTimeMs?: number): ParsedSearchResult => {
    if (!result) {
        return {};
    }
    try {
        const parsed = JSON.parse(result);
        if (!parsed || typeof parsed !== "object") {
            return {};
        }

        const output: ParsedSearchResult = {
            searchTimeMs: fallbackSearchTimeMs,
        };
        if (isRecord(parsed)) {
            applyStructuredSearchPayload(parsed, output);
        }

        const contentParts = getToolContentParts(parsed);
        if (contentParts.length > 0) {
            for (const part of contentParts) {
                if (part.type === "json" && "json" in part) {
                    applyStructuredSearchPayload(part.json, output);
                }
                if (
                    part.type === "text" &&
                    typeof part.text === "string" &&
                    typeof output.resultCount !== "number"
                ) {
                    try {
                        applyStructuredSearchPayload(JSON.parse(part.text), output);
                    } catch {
                        // Markdown / HTML mode: estimate count from link patterns.
                        const estimated = estimateResultCountFromMarkdown(part.text);
                        if (typeof estimated === "number") {
                            output.resultCount = estimated;
                        }
                    }
                }
            }
        } else {
            applyStructuredSearchPayload(parsed, output);
            if (typeof output.resultCount !== "number" && isRecord(parsed) && typeof parsed.text === "string") {
                try {
                    applyStructuredSearchPayload(JSON.parse(parsed.text), output);
                } catch {
                    const estimated = estimateResultCountFromMarkdown(parsed.text);
                    if (typeof estimated === "number") {
                        output.resultCount = estimated;
                    }
                }
            }
        }

        return output;
    } catch {
        return {};
    }
};

const SearchWebToolCall: React.FC<McpToolComponentProps> = (props) => {
    const query = useMemo(() => {
        const value = parseToolParameters(props.parameters).query;
        return typeof value === "string" ? value.trim() : "";
    }, [props.parameters]) || "未指定关键词";
    const call = useWebToolCall(props, "SearchWebToolCall");
    const parsedResult = useMemo(
        () => parseSearchResult(call.result, call.elapsedMs),
        [call.result, call.elapsedMs],
    );

    const meta: WebToolMeta[] = [];
    if (call.state === "success") {
        if (parsedResult.engine) {
            meta.push({ key: "engine", value: parsedResult.engine, title: `搜索引擎：${parsedResult.engine}` });
        }
        if (typeof parsedResult.resultCount === "number") {
            meta.push({ key: "count", value: `${parsedResult.resultCount} 条`, title: `结果数量：${parsedResult.resultCount} 条` });
        }
        if (typeof parsedResult.searchTimeMs === "number") {
            const duration = formatDurationSeconds(parsedResult.searchTimeMs);
            meta.push({ key: "time", value: duration, title: `搜索耗时：${duration}` });
        }
    }

    return (
        <WebToolCard
            kind="search"
            title={call.maskedParameters ?? query}
            state={call.state}
            meta={meta}
            error={call.error}
            shine={call.shine}
            animateEntrance={call.animateEntrance}
            actions={call.actions}
            onOpen={call.onOpen}
            openLabel="在侧栏中定位搜索详情"
        />
    );
};

export default SearchWebToolCall;
