import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export type RustHighlightFn = (lang: string, code: string, isDark: boolean, themeHint?: string) => Promise<string>;

const MAX_HIGHLIGHT_CACHE_ENTRIES = 200;
const highlightResultCache = new Map<string, string>();
const inFlightHighlightRequests = new Map<string, Promise<string>>();

// 两个独立的 32 位哈希拼成 64 位，碰撞会直接显示成别的代码，单个 32 位不够稳。
function hashHighlightCode(code: string): string {
    let fnv = 2166136261;
    let djb = 5381;
    for (let index = 0; index < code.length; index += 1) {
        const charCode = code.charCodeAt(index);
        fnv ^= charCode;
        fnv = Math.imul(fnv, 16777619);
        djb = (Math.imul(djb, 33) + charCode) | 0;
    }
    return `${(fnv >>> 0).toString(36)}.${(djb >>> 0).toString(36)}`;
}

export function buildHighlightCacheKey(
    lang: string,
    code: string,
    isDark: boolean,
    themeHint?: string,
): string {
    return [
        lang,
        isDark ? "1" : "0",
        themeHint ?? "",
        String(code.length),
        hashHighlightCode(code),
        code.slice(0, 24),
        code.slice(-24),
    ].join("\u0000");
}

function storeHighlightResult(key: string, html: string) {
    if (highlightResultCache.has(key)) {
        highlightResultCache.delete(key);
    }
    highlightResultCache.set(key, html);

    if (highlightResultCache.size <= MAX_HIGHLIGHT_CACHE_ENTRIES) {
        return;
    }

    const oldestKey = highlightResultCache.keys().next().value;
    if (oldestKey) {
        highlightResultCache.delete(oldestKey);
    }
}

export function getCachedRustHighlight(
    lang: string, code: string, isDark: boolean, themeHint?: string,
): string | undefined {
    return highlightResultCache.get(buildHighlightCacheKey(lang, code, isDark, themeHint));
}

export function useRustHighlight(): RustHighlightFn {
    return useCallback(async (lang, code, isDark, themeHint) => {
        const cacheKey = buildHighlightCacheKey(lang, code, isDark, themeHint);
        const cached = highlightResultCache.get(cacheKey);
        if (cached !== undefined) {
            return cached;
        }

        const existingRequest = inFlightHighlightRequests.get(cacheKey);
        if (existingRequest) {
            return existingRequest;
        }

        const request = invoke<string>("highlight_code", {
            lang,
            code,
            isDark,
            themeHint: themeHint ?? null,
        })
            .then((result) => {
                storeHighlightResult(cacheKey, result);
                inFlightHighlightRequests.delete(cacheKey);
                return result;
            })
            .catch((error) => {
                inFlightHighlightRequests.delete(cacheKey);
                throw error;
            });

        inFlightHighlightRequests.set(cacheKey, request);
        return request;
    }, []);
}
