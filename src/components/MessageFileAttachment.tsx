import React, { useMemo } from "react";
import { detectOfficeExtension, isOfficeFileName, openOfficePreviewModal } from "@/utils/officeFile";
import { OfficeFormatIcon } from "@/components/office/OfficePreviewPane";

interface MessageFileAttachmentProps {
    title?: string;
    name?: string;
    attachment_url?: string;
    attachment_id?: number;
}

const MessageFileAttachment: React.FC<MessageFileAttachmentProps> = (props) => {
    const { title, name, attachment_url, attachment_id } = props;

    const displayName = useMemo(() => {
        const raw =
            name ||
            attachment_url ||
            "";
        const withoutPrefix = raw.startsWith("user-content-")
            ? raw.replace(/^user-content-/, "")
            : raw;
        const baseName = withoutPrefix.split(/[\\/]/).pop() || withoutPrefix;
        return baseName;
    }, [name, attachment_url]);

    const displayTitle = title || displayName || "附件";
    const officeExt = useMemo(
        () => detectOfficeExtension(displayName) ?? detectOfficeExtension(attachment_url),
        [displayName, attachment_url],
    );
    const isOffice = isOfficeFileName(displayName) || isOfficeFileName(attachment_url);

    const handleClick = () => {
        if (isOffice) {
            openOfficePreviewModal({
                title: displayName,
                extension: officeExt,
                source: {
                    attachmentId: attachment_id,
                    localPath: attachment_url,
                },
            });
        }
    };

    return (
        <div
            onClick={handleClick}
            className="py-3 px-4 bg-slate-50 text-gray-700 border border-gray-200 rounded-lg inline-flex items-center gap-2 cursor-pointer mt-2 text-xs transition-all duration-200 hover:bg-slate-100 hover:border-slate-300 hover:-translate-y-0.5 hover:shadow-lg"
            title={isOffice ? `点击预览 ${displayTitle}` : displayTitle}
        >
            {isOffice && <OfficeFormatIcon ext={officeExt} className="h-4 w-4 shrink-0 text-muted-foreground" />}
            <span>文件名称：{displayName}</span>
        </div>
    );
};

export default MessageFileAttachment;
