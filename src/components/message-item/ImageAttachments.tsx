import React, { useCallback } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { ExternalLink } from 'lucide-react';
import { toast } from 'sonner';
import { useIsMobilePlatform } from '@/hooks/use-platform';
import { fileErrorMessage, outputMobileFile } from '@/services/mobileFileService';
import { detectOfficeExtension, isOfficeFileName, officeFormatLabel, openOfficePreviewModal } from '@/utils/officeFile';
import { OfficeFormatIcon } from '@/components/office/OfficePreviewPane';

const MOBILE_IMAGE_EXTENSIONS: Record<string, string> = {
    'image/png': 'png',
    'image/jpeg': 'jpg',
    'image/gif': 'gif',
    'image/webp': 'webp',
};

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
    messageContent?: string;
}

const ImageAttachments: React.FC<ImageAttachmentsProps> = ({
    attachments,
    conversationId,
    messageId,
    messageContent,
}) => {
    const isMobilePlatform = useIsMobilePlatform();

    const handleOpenImage = useCallback(
        async (attachment: Attachment) => {
            const imageData = attachment.attachment_content || attachment.attachment_url;
            if (!imageData) {
                return;
            }

            try {
                if (isMobilePlatform) {
                    // 移动端 open_image（open::that）不可用：读出图片字节，走 FileProvider 交给系统应用打开
                    const response = await fetch(imageData);
                    if (!response.ok) {
                        throw new Error(`读取图片失败：HTTP ${response.status}`);
                    }
                    const blob = await response.blob();
                    const ext = MOBILE_IMAGE_EXTENSIONS[blob.type];
                    if (!ext) {
                        throw new Error(`不支持打开图片格式：${blob.type || '未知'}`);
                    }
                    await outputMobileFile('open', new Uint8Array(await blob.arrayBuffer()), `image.${ext}`);
                    return;
                }
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
                toast.error(`图片打开失败：${fileErrorMessage(error)}`);
            }
        },
        [conversationId, messageId, isMobilePlatform]
    );

    if (!attachments?.length) {
        return null;
    }

    const imageAttachments = attachments.filter(
        (attachment) =>
            attachment.attachment_type === 'Image' &&
            Boolean(attachment.attachment_content || attachment.attachment_url)
    );

    const officeAttachments = attachments.filter(
        (attachment) =>
            (attachment.attachment_type === 'Word' ||
                attachment.attachment_type === 'Excel' ||
                attachment.attachment_type === 'PowerPoint' ||
                isOfficeFileName(attachment.attachment_url)) &&
            Boolean(attachment.id || attachment.attachment_url),
    );

    const fileAttachments = attachments.filter(attachment => {
        if (attachment.attachment_type === 'Text' && messageContent !== undefined) {
            return !messageContent.includes(`<fileattachment name="${attachment.attachment_url ?? '附件'}">`);
        }
        return attachment.attachment_type === 'File' || attachment.attachment_type === 'PDF';
    });
    if (!imageAttachments.length && !officeAttachments.length && !fileAttachments.length) {
        return null;
    }

    const useGridLayout = imageAttachments.length > 1;
    // 移动端气泡可能窄于 300px，避免撑破气泡
    const widthClass = isMobilePlatform ? 'w-full max-w-[300px]' : 'w-[300px]';

    return (
        <div className="flex flex-col gap-2">
            {imageAttachments.length > 0 && (
                <div
                    className={
                        useGridLayout
                            ? `mt-3 grid ${widthClass} grid-cols-2 gap-2`
                            : `mt-3 flex ${widthClass} flex-col gap-2`
                    }
                >
                    {imageAttachments.map((attachment, index) => {
                        const imageSrc = attachment.attachment_content || attachment.attachment_url;
                        if (!imageSrc) {
                            return null;
                        }
                        const isGenerating = attachment.attachment_url?.startsWith('aipp://image-generation/pending/');

                        if (isGenerating) {
                            return (
                                <div
                                    key={attachment.id ?? `image-generating-${index}`}
                                    className="relative aspect-square w-full overflow-hidden rounded-xl border border-border bg-muted/40"
                                    role="status"
                                    aria-label="正在生成图片"
                                >
                                    <div className="flex h-full items-center justify-center text-sm text-muted-foreground">正在生成中…</div>
                                </div>
                            );
                        }

                        return (
                            <button
                                key={attachment.id ?? attachment.attachment_url ?? `image-${index}`}
                                type="button"
                                className={
                                    useGridLayout
                                        ? "group relative aspect-square w-full overflow-hidden rounded-xl border border-border bg-muted/20 text-left transition-colors hover:bg-muted/30"
                                        : "group relative aspect-[4/3] w-full overflow-hidden rounded-xl border border-border bg-muted/20 text-left transition-colors hover:bg-muted/30"
                                }
                                onClick={() => void handleOpenImage(attachment)}
                                title="点击使用系统默认程序打开图片"
                                aria-label="点击使用系统默认程序打开图片"
                            >
                                <img
                                    className="block h-full w-full object-contain"
                                    src={imageSrc}
                                    alt={attachment.attachment_url || 'Message attachment'}
                                    decoding="async"
                                />
                                <div className={`pointer-events-none absolute inset-x-0 bottom-0 flex items-center justify-between bg-gradient-to-t from-black/70 via-black/30 to-transparent px-3 py-2 text-white transition-opacity ${isMobilePlatform ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'}`}>
                                    <span className="truncate text-xs">
                                        {attachment.attachment_url || '打开图片'}
                                    </span>
                                    <ExternalLink className="h-3.5 w-3.5 flex-shrink-0" />
                                </div>
                            </button>
                        );
                    })}
                </div>
            )}
            {fileAttachments.map((a, index) => (
                <span key={a.id ?? index} className="w-fit max-w-full truncate rounded-md border px-2 py-1 text-sm" title={a.attachment_url}>
                    {a.attachment_url?.split(/[\\\\/]/).pop() || '文件附件'}
                </span>
            ))}
            {officeAttachments.length > 0 && (
                <div className="mt-2 flex flex-wrap gap-2">
                    {officeAttachments.map((attachment, index) => {
                        const rawName = attachment.attachment_url || `document-${attachment.id ?? index}`;
                        const baseName = rawName.split(/[\\/]/).pop() || rawName;
                        const ext = detectOfficeExtension(baseName) ?? (
                            attachment.attachment_type === 'Word'
                                ? 'docx'
                                : attachment.attachment_type === 'Excel'
                                  ? 'xlsx'
                                  : attachment.attachment_type === 'PowerPoint'
                                    ? 'pptx'
                                    : null
                        );
                        return (
                            <button
                                key={attachment.id ?? attachment.attachment_url ?? `office-${index}`}
                                type="button"
                                onClick={() =>
                                    openOfficePreviewModal({
                                        title: baseName,
                                        extension: ext,
                                        source: {
                                            attachmentId: attachment.id,
                                            localPath: attachment.attachment_url,
                                        },
                                        conversationId,
                                    })
                                }
                                className="inline-flex min-h-10 max-w-xs items-center gap-2 rounded-lg border border-border bg-card px-3 py-2 text-left text-xs text-card-foreground transition-colors hover:bg-muted/60"
                                title={`点击预览 ${baseName}`}
                            >
                                <OfficeFormatIcon ext={ext} className="h-4 w-4 shrink-0 text-muted-foreground" />
                                <div className="min-w-0 flex-1">
                                    <div className="truncate font-medium">{baseName}</div>
                                    <div className="text-[11px] text-muted-foreground">{officeFormatLabel(ext)} · 点击预览</div>
                                </div>
                            </button>
                        );
                    })}
                </div>
            )}
        </div>
    );
};

export default ImageAttachments;
