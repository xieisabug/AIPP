import { useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";

export type RustHighlightFn = (lang: string, code: string, isDark: boolean, themeHint?: string) => Promise<string>;

export function useRustHighlight(): RustHighlightFn {
    return useCallback(async (lang, code, isDark, themeHint) => {
        return await invoke<string>("highlight_code", {
            lang,
            code,
            isDark: isDark as unknown as any,
            themeHint: (themeHint ?? null) as unknown as any,
        });
    }, []);
}
