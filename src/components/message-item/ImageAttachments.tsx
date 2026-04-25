import React, { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ExternalLink } from 'lucide-react';

interface Attachment {
    id?: number;
    attachment_url?: string;
    attachment_content?: string;
    attachment_type: string;
}

interface ImageAttachmentsProps {
    attachments?: Attachment[];
    conversationId?: number | null;
    messageId?: number;
}

const ImageAttachments: React.FC<ImageAttachmentsProps> = ({
    attachments,
    conversationId,
    messageId,
}) => {
    if (!attachments?.length) {
        return null;
    }

    const imageAttachments = attachments.filter(
        (attachment) =>
            attachment.attachment_type === 'Image' &&
            Boolean(attachment.attachment_content || attachment.attachment_url)
    );

    if (!imageAttachments.length) {
        return null;
    }

    const handleOpenImage = useCallback(
        async (attachment: Attachment) => {
            const imageData = attachment.attachment_content || attachment.attachment_url;
            if (!imageData) {
                return;
            }

            try {
                await invoke('open_image', {
                    imageData,
                    conversationId:
                        conversationId !== undefined && conversationId !== null
                            ? String(conversationId)
                            : undefined,
                    messageId: String(attachment.id ?? messageId ?? ''),
                });
            } catch (error) {
                console.error('Open image failed', error);
            }
        },
        [conversationId, messageId]
    );

    return (
        <div className="mt-3 flex w-[300px] flex-col gap-2">
            {imageAttachments.map((attachment, index) => {
                const imageSrc = attachment.attachment_content || attachment.attachment_url;
                if (!imageSrc) {
                    return null;
                }

                return (
                    <button
                        key={attachment.id ?? attachment.attachment_url ?? `image-${index}`}
                        type="button"
                        className="group relative overflow-hidden rounded-xl border border-border bg-muted/20 text-left transition-colors hover:bg-muted/30"
                        onClick={() => void handleOpenImage(attachment)}
                        title="点击使用系统默认程序打开图片"
                        aria-label="点击使用系统默认程序打开图片"
                    >
                        <img
                            className="block max-h-[320px] w-full h-auto object-contain"
                            src={imageSrc}
                            alt={attachment.attachment_url || 'Message attachment'}
                        />
                        <div className="pointer-events-none absolute inset-x-0 bottom-0 flex items-center justify-between bg-gradient-to-t from-black/70 via-black/30 to-transparent px-3 py-2 text-white opacity-0 transition-opacity group-hover:opacity-100">
                            <span className="truncate text-xs">
                                {attachment.attachment_url || '打开图片'}
                            </span>
                            <ExternalLink className="h-3.5 w-3.5 flex-shrink-0" />
                        </div>
                    </button>
                );
            })}
        </div>
    );
};

export default ImageAttachments;
