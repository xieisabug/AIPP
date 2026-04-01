import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export type RustHighlightFn = (lang: string, code: string, isDark: boolean, themeHint?: string) => Promise<string>;

const MAX_HIGHLIGHT_CACHE_ENTRIES = 200;
const highlightResultCache = new Map<string, string>();
const inFlightHighlightRequests = new Map<string, Promise<string>>();

function buildHighlightCacheKey(
    lang: string,
    code: string,
    isDark: boolean,
    themeHint?: string,
): string {
    return JSON.stringify([lang, code, isDark, themeHint ?? null]);
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
