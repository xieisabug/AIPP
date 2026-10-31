import { Dispatch, SetStateAction, useCallback, useRef, useState } from "react";
import { useMobileDraftPersistence } from "./useMobileDraftPersistence";

/** Desktop session drafts; mobile persists in the native app-local DB, never browser storage. */
export function useConversationDraft(conversationId: string): [string, Dispatch<SetStateAction<string>>] {
    const drafts = useRef(new Map<string, string>());
    const [, redraw] = useState(0);
    const persist = useMobileDraftPersistence(conversationId, content => {
        drafts.current.set(conversationId, content);
        redraw(version => version + 1);
    });
    const setDraft = useCallback<Dispatch<SetStateAction<string>>>(value => {
        const previous = drafts.current.get(conversationId) ?? "";
        const next = typeof value === "function" ? value(previous) : value;
        drafts.current.set(conversationId, next);
        persist(next);
        redraw(version => version + 1);
    }, [conversationId, persist]);
    return [drafts.current.get(conversationId) ?? "", setDraft];
}
