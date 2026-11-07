import { useCallback, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { toast } from "sonner";
import { useIsMobilePlatform } from "./use-platform";
import { AttachmentType, FileInfo } from "@/data/Conversation";

export interface MobileDraftAttachmentDto {
    id: number;
    name: string;
    path: string;
    attachment_type: number;
    thumbnail?: string | null;
}

export interface MobileDraftMissingAttachmentDto {
    id: number;
    name: string;
}

export interface MobileDraftDto {
    content: string;
    attachments: MobileDraftAttachmentDto[];
    /** 草稿记录过但这次无法恢复的附件，需要提示用户重新选择。旧后端可能不返回。 */
    missing_attachments?: MobileDraftMissingAttachmentDto[];
}

export function draftMissingAttachments(draft: Partial<MobileDraftDto> | null | undefined): MobileDraftMissingAttachmentDto[] {
    const missing = Array.isArray(draft?.missing_attachments) ? draft.missing_attachments : [];
    return missing.filter((item) => Number.isFinite(item?.id) && item.id > 0)
        .map((item) => ({ id: item.id, name: item.name || `attachment-${item.id}` }));
}

const writes = new Map<string, Promise<unknown>>();

function toFileInfo(attachment: MobileDraftAttachmentDto): FileInfo | null {
    if (!Number.isFinite(attachment.id) || attachment.id <= 0) return null;
    const type =
        attachment.attachment_type === AttachmentType.Image
            ? AttachmentType.Image
            : AttachmentType.Text;
    return {
        id: attachment.id,
        name: attachment.name || `attachment-${attachment.id}`,
        path: attachment.path || attachment.name || `attachment-${attachment.id}`,
        type,
        thumbnail: attachment.thumbnail ?? undefined,
    };
}

export function draftAttachmentsToFileInfo(attachments: MobileDraftAttachmentDto[]): FileInfo[] {
    return attachments.flatMap((attachment) => {
        const file = toFileInfo(attachment);
        return file ? [file] : [];
    });
}

export interface DraftRestoreOptions {
    preserveAttachments?: boolean;
}

type DraftPayload = { content: string; attachmentIds: number[] };

/**
 * Persist mobile drafts only after the initial get/hydrate finishes so an early
 * saveContent cannot bump revision with attachmentIds:[] and skip restore of
 * real attachment ids (peer review 2026-11-03 / CAP #164).
 */
export function useMobileDraftPersistence(
    key: string,
    restore: (draft: MobileDraftDto, options?: DraftRestoreOptions) => void,
    onReady?: () => void,
) {
    const mobile = useIsMobilePlatform();
    const contentRevisions = useRef(new Map<string, number>());
    const attachmentRevisions = useRef(new Map<string, number>());
    const hydrateReady = useRef(new Map<string, boolean>());
    const pendingWrite = useRef(new Map<string, boolean>());
    const latestRestore = useRef(restore);
    latestRestore.current = restore;
    const latestReady = useRef(onReady);
    latestReady.current = onReady;
    const latestPayload = useRef(new Map<string, DraftPayload>());

    const persist = useCallback(
        (conversationKey: string, payload: DraftPayload) => {
            const request = Promise.resolve(writes.get(conversationKey))
                .then(() =>
                    invoke("save_mobile_draft", {
                        conversationKey,
                        content: payload.content,
                        attachmentIds: payload.attachmentIds,
                    }),
                )
                .catch((error) =>
                    toast.error(`保存本机草稿失败，当前输入仍保留: ${String(error)}`),
                );
            writes.set(conversationKey, request);
            void request.finally(() => {
                if (writes.get(conversationKey) === request) writes.delete(conversationKey);
            });
        },
        [],
    );

    useEffect(() => {
        if (!mobile) {
            hydrateReady.current.set(key, true);
            latestReady.current?.();
            return;
        }
        let disposed = false;
        hydrateReady.current.set(key, false);
        pendingWrite.current.set(key, false);
        const contentRevision = contentRevisions.current.get(key) ?? 0;
        const attachmentRevision = attachmentRevisions.current.get(key) ?? 0;
        void Promise.resolve(writes.get(key))
            .then(() => invoke<MobileDraftDto>("get_mobile_draft", { conversationKey: key }))
            .then((draft) => {
                if (disposed) return;
                const attachments = Array.isArray(draft?.attachments) ? draft.attachments : [];
                const content = typeof draft?.content === "string" ? draft.content : "";
                const attachmentIds = attachments.map((item) => item.id).filter((id) => id > 0);
                const missing_attachments = draftMissingAttachments(draft);
                const contentChanged = (contentRevisions.current.get(key) ?? 0) !== contentRevision;
                const attachmentsChanged = (attachmentRevisions.current.get(key) ?? 0) !== attachmentRevision;
                const current = latestPayload.current.get(key);
                const restoredContent = contentChanged ? current?.content ?? "" : content;
                latestPayload.current.set(key, {
                    content: restoredContent,
                    // An explicit clear is an edit too; never infer it from array length.
                    attachmentIds: attachmentsChanged ? current?.attachmentIds ?? [] : attachmentIds,
                });
                if (attachmentsChanged) {
                    // The UI already owns the newly selected files, including their metadata.
                    // Restore untouched text only; stale attachment DTOs must not replace them.
                    latestRestore.current({ content: restoredContent, attachments: [] }, { preserveAttachments: true });
                } else if (!contentChanged || attachments.length > 0 || missing_attachments.length > 0) {
                    latestRestore.current({ content: restoredContent, attachments, missing_attachments });
                }
            })
            .catch((error) => {
                if (!disposed) toast.error(`恢复本机草稿失败: ${String(error)}`);
            })
            .finally(() => {
                if (disposed) return;
                hydrateReady.current.set(key, true);
                if (pendingWrite.current.get(key)) {
                    pendingWrite.current.set(key, false);
                    const payload = latestPayload.current.get(key);
                    if (payload) persist(key, payload);
                }
                latestReady.current?.();
            });
        return () => {
            disposed = true;
        };
    }, [key, mobile, persist]);

    const save = useCallback(
        (next: { content?: string; attachmentIds?: number[] }) => {
            if (!mobile) return;
            const previous = latestPayload.current.get(key) ?? {
                content: "",
                attachmentIds: [] as number[],
            };
            const payload = {
                content: next.content ?? previous.content,
                attachmentIds: next.attachmentIds ?? previous.attachmentIds,
            };
            latestPayload.current.set(key, payload);
            if (next.content !== undefined) {
                contentRevisions.current.set(key, (contentRevisions.current.get(key) ?? 0) + 1);
            }
            if (next.attachmentIds !== undefined) {
                attachmentRevisions.current.set(key, (attachmentRevisions.current.get(key) ?? 0) + 1);
            }
            if (!hydrateReady.current.get(key)) {
                // Gate disk writes until hydrate finishes so we never persist a
                // pre-restore empty attachmentIds that would skip restore.
                pendingWrite.current.set(key, true);
                return;
            }
            persist(key, payload);
        },
        [key, mobile, persist],
    );

    const saveContent = useCallback(
        (content: string) => {
            save({ content });
        },
        [save],
    );

    const saveAttachmentIds = useCallback(
        (attachmentIds: number[]) => {
            save({ attachmentIds });
        },
        [save],
    );

    return { saveContent, saveAttachmentIds };
}
