import React, { useMemo } from "react";
import { WebToolCard, formatDurationSeconds, parseToolParameters, useWebToolCall, type WebToolMeta } from "@/components/mcp-tool-components/WebToolCard";
import type { McpToolComponentProps } from "@/services/mcpToolComponentRegistry";

interface ParsedFetchResult {
    fetchTimeMs?: number;
    wordCount?: number;
}

const isRecord = (value: unknown): value is Record<string, unknown> => (
    Boolean(value) && typeof value === "object" && !Array.isArray(value)
);

const isToolContentPart = (value: unknown): value is Record<string, unknown> => (
    isRecord(value) && typeof value.type === "string" && ("text" in value || "json" in value)
);

const numberValue = (value: unknown): number | undefined => (
    typeof value === "number" && Number.isFinite(value) ? Math.max(0, value) : undefined
);

const getToolContentParts = (parsed: unknown): Record<string, unknown>[] => {
    if (Array.isArray(parsed)) {
        return parsed.filter(isToolContentPart);
    }
    if (isRecord(parsed) && Array.isArray(parsed.content)) {
        return parsed.content.filter(isToolContentPart);
    }
    return [];
};

const extractTextFromResult = (result?: string): string => {
    if (!result) {
        return "";
    }
    try {
        const parsed = JSON.parse(result);
        if (!parsed || typeof parsed !== "object") {
            return result;
        }
        const content = getToolContentParts(parsed);
        const texts: string[] = [];
        for (const item of content) {
            if (typeof item.text === "string") {
                texts.push(item.text);
            }
        }
        if (texts.length > 0) {
            return texts.join("\n").trim();
        }
        return typeof parsed.text === "string" ? parsed.text.trim() : "";
    } catch {
        return result;
    }
};

const countWords = (text: string): number => {
    if (!text) {
        return 0;
    }
    // 匹配连续中文字符作为词，再匹配非空白西文词
    const cjkMatches = text.match(/[一-龥]/g) || [];
    const wordMatches = text.match(/[a-zA-Z0-9_]+/g) || [];
    return cjkMatches.length + wordMatches.length;
};

const applyStructuredFetchPayload = (payload: unknown, output: ParsedFetchResult): boolean => {
    if (!isRecord(payload)) {
        return false;
    }

    const fetchTimeMs = numberValue(payload.fetch_time_ms)
        ?? numberValue(payload.elapsed_ms)
        ?? numberValue(payload.duration_ms);
    if (typeof fetchTimeMs === "number") {
        output.fetchTimeMs = fetchTimeMs;
    }

    return typeof fetchTimeMs === "number";
};

const parseFetchResult = (result?: string, fallbackFetchTimeMs?: number): ParsedFetchResult => {
    const output: ParsedFetchResult = {
        fetchTimeMs: fallbackFetchTimeMs,
    };
    const text = extractTextFromResult(result);
    if (text) {
        output.wordCount = countWords(text);
    }

    if (result) {
        try {
            const parsed = JSON.parse(result);
            applyStructuredFetchPayload(parsed, output);
            for (const part of getToolContentParts(parsed)) {
                if (part.type === "json" && "json" in part) {
                    applyStructuredFetchPayload(part.json, output);
                }
            }
        } catch {
            // Raw text result only contributes word count.
        }
    }
    return output;
};

const FetchUrlToolCall: React.FC<McpToolComponentProps> = (props) => {
    const url = useMemo(() => {
        const value = parseToolParameters(props.parameters).url;
        return typeof value === "string" ? value.trim() : "";
    }, [props.parameters]) || "未指定 URL";
    const call = useWebToolCall(props, "FetchUrlToolCall");
    const parsedResult = useMemo(
        () => parseFetchResult(call.result, call.elapsedMs),
        [call.result, call.elapsedMs],
    );

    const meta: WebToolMeta[] = [];
    if (call.state === "success") {
        if (typeof parsedResult.wordCount === "number") {
            meta.push({ key: "words", value: `${parsedResult.wordCount} 字`, title: `参考文档：${parsedResult.wordCount} 字` });
        }
        if (typeof parsedResult.fetchTimeMs === "number") {
            const duration = formatDurationSeconds(parsedResult.fetchTimeMs);
            meta.push({ key: "time", value: duration, title: `抓取耗时：${duration}` });
        }
    }

    return (
        <WebToolCard
            kind="fetch"
            title={call.maskedParameters ?? url}
            state={call.state}
            meta={meta}
            error={call.error}
            shine={call.shine}
            animateEntrance={call.animateEntrance}
            actions={call.actions}
            onOpen={call.onOpen}
            openLabel="在侧栏中定位抓取详情"
        />
    );
};

export default FetchUrlToolCall;
