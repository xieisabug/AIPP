import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

export function useSyntectThemes() {
    const [themes, setThemes] = useState<string[] | null>(null);
    const [isLoading, setIsLoading] = useState(false);
    const [error, setError] = useState<string | null>(null);

    const fetchThemes = useCallback(async () => {
        try {
            setIsLoading(true);
            setError(null);
            const list = await invoke<string[]>("list_syntect_themes");
            setThemes(list);
        } catch (e: any) {
            setError(e?.message || String(e));
            setThemes([]);
        } finally {
            setIsLoading(false);
        }
    }, []);

    useEffect(() => {
        fetchThemes();
    }, [fetchThemes]);

    return { themes, isLoading, error, refresh: fetchThemes };
}
