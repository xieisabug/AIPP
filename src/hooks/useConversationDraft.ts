import { createContext, Dispatch, SetStateAction, useCallback, useContext, useRef, useState, useSyncExternalStore } from "react";
import { FileInfo } from "@/data/Conversation";
import {
    draftAttachmentsToFileInfo,
    draftMissingAttachments,
    MobileDraftDto,
    DraftRestoreOptions,
    MobileDraftMissingAttachmentDto,
    useMobileDraftPersistence,
} from "./useMobileDraftPersistence";

export function createConversationDraftStore() {
    const drafts = new Map<string, string>();
    const listeners = new Set<() => void>();
    return {
        get: (id: string) => drafts.get(id) ?? "",
        set(id: string, text: string) {
            if ((drafts.get(id) ?? "") === text) return;
            if (text) drafts.set(id, text);
            else drafts.delete(id);
            listeners.forEach(notify => notify());
        },
        subscribe(notify: () => void) {
            listeners.add(notify);
            return () => { listeners.delete(notify); };
        },
    };
}

export const ConversationDraftContext = createContext<ReturnType<typeof createConversationDraftStore> | null>(null);

/** Desktop session drafts; mobile persists in the native app-local DB, never browser storage. */
export function useConversationDraft(
    conversationId: string,
    onRestoreAttachments?: (files: FileInfo[]) => void,
    onDraftReady?: () => void,
    onMissingAttachments?: (missing: MobileDraftMissingAttachmentDto[]) => void,
): [string, Dispatch<SetStateAction<string>>, (attachmentIds: number[]) => void] {
    const windowDrafts = useContext(ConversationDraftContext);
    const [localDrafts] = useState(createConversationDraftStore);
    const drafts = windowDrafts ?? localDrafts;
    const content = useSyncExternalStore(drafts.subscribe, () => drafts.get(conversationId));
    const restoreAttachments = useRef(onRestoreAttachments);
    restoreAttachments.current = onRestoreAttachments;
    const draftReady = useRef(onDraftReady);
    draftReady.current = onDraftReady;
    const missingAttachments = useRef(onMissingAttachments);
    missingAttachments.current = onMissingAttachments;

    const { saveContent, saveAttachmentIds } = useMobileDraftPersistence(
        conversationId,
        (draft: MobileDraftDto, options?: DraftRestoreOptions) => {
            drafts.set(conversationId, draft.content);
            if (!options?.preserveAttachments) {
                restoreAttachments.current?.(draftAttachmentsToFileInfo(draft.attachments));
            }
            const missing = draftMissingAttachments(draft);
            if (missing.length > 0) missingAttachments.current?.(missing);
        },
        () => draftReady.current?.(),
    );

    const setDraft = useCallback<Dispatch<SetStateAction<string>>>(
        (value) => {
            const previous = drafts.get(conversationId);
            const next = typeof value === "function" ? value(previous) : value;
            drafts.set(conversationId, next);
            saveContent(next);
        },
        [conversationId, drafts, saveContent],
    );

    return [content, setDraft, saveAttachmentIds];
}
