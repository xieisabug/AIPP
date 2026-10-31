import { Dispatch, SetStateAction, useCallback, useRef, useState } from "react";

/** Keep each conversation draft for the lifetime of this chat view. */
export function useConversationDraft(conversationId: string): [string, Dispatch<SetStateAction<string>>] {
    const drafts = useRef(new Map<string, string>());
    const [, redraw] = useState(0);
    const setDraft = useCallback<Dispatch<SetStateAction<string>>>(value => {
        const previous = drafts.current.get(conversationId) ?? "";
        const next = typeof value === "function" ? value(previous) : value;
        drafts.current.set(conversationId, next);
        redraw(version => version + 1);
    }, [conversationId]);
    return [drafts.current.get(conversationId) ?? "", setDraft];
}
