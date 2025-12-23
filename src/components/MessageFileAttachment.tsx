import React, { useMemo } from "react";

interface MessageFileAttachmentProps {
    title?: string;
    name?: string;
    attachment_url?: string;
}

const MessageFileAttachment: React.FC<MessageFileAttachmentProps> = (props) => {
    const { title, name, attachment_url } = props;

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

    return (
        <div className="py-3 px-4 bg-slate-50 text-gray-700 border border-gray-200 rounded-lg inline-block cursor-pointer mt-2 text-xs transition-all duration-200 hover:bg-slate-100 hover:border-slate-300 hover:-translate-y-0.5 hover:shadow-lg" title={displayTitle}>
            <span>文件名称：{displayName}</span>
        </div>
    );
};

export default MessageFileAttachment;
