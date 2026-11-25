import { useCallback, useEffect, useRef, useState } from "react";
import type { AcpConversationSessionState, AcpSessionConfigOption } from "@/data/Conversation";

interface Draft {
    scope: string;
    mode: string;
    plan: boolean;
    awaitingSession: boolean;
}

export function useClaudeComposerPermissions(
    scope: string,
    configuredMode: string,
    session: AcpConversationSessionState | null,
    save: (option: AcpSessionConfigOption, value: string) => Promise<boolean>,
) {
    const [stored, setDraft] = useState<Draft | null>(null);
    const draft = stored?.scope === scope ? stored : null;
    useEffect(() => { setDraft(previous => previous?.scope === scope ? previous : null); }, [scope]);
    const option = session?.session_id ? session.config_options.find(o => o.id === "permission_mode") : undefined;
    const current = option?.current_value ?? session?.permission_mode ?? configuredMode;
    const plan = option ? current === "plan" : (draft?.plan ?? (current === "plan"));
    const baseMode = current === "plan" ? (configuredMode === "plan" ? "default" : configuredMode) : current;
    const mode = option && !plan ? current : draft?.mode ?? baseMode;
    const choose = useCallback(async (mode: string, plan: boolean) => {
        if (option && !await save(option, plan ? "plan" : mode)) return false;
        setDraft({ scope, mode, plan, awaitingSession: !option });
        return true;
    }, [option, save, scope]);

    // A choice made during connection setup becomes a session selection as soon
    // as the actor advertises its options. The first prompt can use it immediately.
    const binding = useRef<Draft | null>(null);
    useEffect(() => {
        if (!option || !draft?.awaitingSession || binding.current === draft) return;
        binding.current = draft;
        void save(option, draft.plan ? "plan" : draft.mode).then(saved => {
            setDraft(previous => {
                if (previous !== draft) return previous;
                return saved ? { ...draft, awaitingSession: false } : null;
            });
        });
    }, [draft, option, save]);
    const initialOverride = !option && draft ? (draft.plan ? "plan" : draft.mode) : undefined;
    return { mode, plan, choose, initialOverride };
}
