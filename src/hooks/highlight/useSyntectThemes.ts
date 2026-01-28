import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export interface SyntectThemeInfo {
    name: string;
    is_dark: boolean;
}

export function useSyntectThemes() {
    const [themes, setThemes] = useState<string[] | null>(null);
    const [themeInfo, setThemeInfo] = useState<SyntectThemeInfo[] | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const fetchThemes = useCallback(async () => {
        try {
            setIsLoading(true);
            setError(null);
            const list = await invoke<SyntectThemeInfo[]>("list_syntect_themes");
            setThemeInfo(list);
            setThemes(list.map((item) => item.name));
        } catch (e: any) {
            setError(e?.message || String(e));
            setThemes([]);
            setThemeInfo([]);
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        fetchThemes();
    }, [fetchThemes]);

    return { themes, themeInfo, isLoading, error, refresh: fetchThemes };
}
