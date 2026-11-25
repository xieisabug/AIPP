import { useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { AcpConversationSessionState } from "@/data/Conversation";

export function useNextAgentPermissions(state: AcpConversationSessionState | null) {
    const conversationId = state?.conversation_id;
    const driver = state?.agent_kind;
    const sessionId = state?.session_id;
    const scope = JSON.stringify([conversationId, driver, sessionId]);
    const [selection, setSelection] = useState<{scope: string; values: Record<string, string>} | null>(null);

    useEffect(() => {
        if (!conversationId || !sessionId || !driver || driver === "codex_app_server") return;
        let closed = false;
        let revision = 0;
        const refresh = async () => {
            const request = ++revision;
            const values = await invoke<Record<string, string>>("get_agent_next_permissions", { conversationId, driver, sessionId });
            if (!closed && request === revision) setSelection({ scope, values });
        };
        const report = (error: unknown) => console.error("读取下轮 Agent 权限失败", error);
        const listener = listen<number>("agent-next-permissions-changed", event => {
            if (event.payload === conversationId) void refresh().catch(report);
        });
        void listener.then(() => { if (!closed) return refresh(); }).catch(report);
        return () => { closed = true; void listener.then(remove => remove()).catch(report); };
    }, [conversationId, driver, sessionId, scope]);

    return useMemo(() => {
        if (!state || selection?.scope !== scope || !selection.values) return state;
        return { ...state, config_options: state.config_options.map(option => {
            const value = selection.values[option.id];
            if (typeof value !== "string" || value === option.current_value) return option;
            return { ...option, current_value: value, description: "已选择，将在下一条用户消息发送前应用" };
        }) };
    }, [state, scope, selection]);
}
