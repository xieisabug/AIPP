import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useDataRefresh } from "./useDataRefresh";

export type WebMode = "off" | "search" | "deep";
const storageKey = (id: string) => `deepseek-web-mode:${id}`;

export function rememberWebMode(id: string, mode: WebMode) {
    if (id) localStorage.setItem(storageKey(id), mode);
}

export function useDeepSeekWeb(assistantId: number | null | undefined, conversationId: string, model: string) {
    const identity = JSON.stringify([assistantId, conversationId, model]);
    const [capability, setCapability] = useState<{identity: string; native: boolean}>();
    const [selection, setSelection] = useState<{identity: string; mode: WebMode}>();
    useDataRefresh({
        events: ["assistant_list_changed", "llm_provider_changed", "mcp_state_changed"], identity, initial: true,
        refresh: async current => {
            const available = assistantId && assistantId > 0 && await invoke<{native_tools: boolean} | null>("get_deepseek_web_capability",
                { assistantId, overrideModelId: model || null, conversationId: Number(conversationId) || null });
            if (current()) setCapability(available ? {identity, native: available.native_tools} : undefined);
        },
        onError: () => setCapability(undefined),
    });
    if (capability?.identity !== identity) return undefined;
    const saved = conversationId ? localStorage.getItem(storageKey(conversationId)) : null;
    const mode = selection?.identity === identity ? selection.mode : saved === "search" || saved === "deep" ? saved : "off";
    return { mode, requiresThinking: capability.native, change(next: WebMode) {
        setSelection({ identity, mode: next });
        rememberWebMode(conversationId, next);
    }};
}
