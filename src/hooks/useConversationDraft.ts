import { Dispatch, SetStateAction, useCallback, useRef, useState } from "react";
import { FileInfo } from "@/data/Conversation";
import {
    draftAttachmentsToFileInfo,
    draftMissingAttachments,
    MobileDraftDto,
    MobileDraftMissingAttachmentDto,
    useMobileDraftPersistence,
} from "./useMobileDraftPersistence";

/** Desktop session drafts; mobile persists in the native app-local DB, never browser storage. */
export function useConversationDraft(
    conversationId: string,
    onRestoreAttachments?: (files: FileInfo[]) => void,
    onDraftReady?: () => void,
    onMissingAttachments?: (missing: MobileDraftMissingAttachmentDto[]) => void,
): [string, Dispatch<SetStateAction<string>>, (attachmentIds: number[]) => void] {
    const drafts = useRef(new Map<string, string>());
    const [, redraw] = useState(0);
    const restoreAttachments = useRef(onRestoreAttachments);
    restoreAttachments.current = onRestoreAttachments;
    const draftReady = useRef(onDraftReady);
    draftReady.current = onDraftReady;
    const missingAttachments = useRef(onMissingAttachments);
    missingAttachments.current = onMissingAttachments;

    const { saveContent, saveAttachmentIds } = useMobileDraftPersistence(
        conversationId,
        (draft: MobileDraftDto) => {
            drafts.current.set(conversationId, draft.content);
            restoreAttachments.current?.(draftAttachmentsToFileInfo(draft.attachments));
            const missing = draftMissingAttachments(draft);
            if (missing.length > 0) missingAttachments.current?.(missing);
            redraw((version) => version + 1);
        },
        () => draftReady.current?.(),
    );

    const setDraft = useCallback<Dispatch<SetStateAction<string>>>(
        (value) => {
            const previous = drafts.current.get(conversationId) ?? "";
            const next = typeof value === "function" ? value(previous) : value;
            drafts.current.set(conversationId, next);
            saveContent(next);
            redraw((version) => version + 1);
        },
        [conversationId, saveContent],
    );

    return [drafts.current.get(conversationId) ?? "", setDraft, saveAttachmentIds];
}
