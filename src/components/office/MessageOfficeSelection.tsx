import React from "react";
import { detectOfficeExtension, openOfficePreviewModal } from "@/utils/officeFile";
import { OfficeFormatIcon } from "./OfficePreviewPane";

interface MessageOfficeSelectionProps {
    refid?: string;
    file?: string;
    format?: string;
    location?: string;
    excerpt?: string;
}

/**
 * 用户消息里的 `<officeselection>`：交给助手的 Office 选区。只显示文件、位置与摘录；
 * 完整原文在标签内容里给模型看。点击按引用号打开这份文件。
 * 用 span 骨架：气泡的 `.prose` 会按标签名给 p/div 加外边距。
 */
const MessageOfficeSelection: React.FC<MessageOfficeSelectionProps> = ({ refid, file, format, location, excerpt }) => {
    const name = file || "Office 文件";
    const ext = detectOfficeExtension(name) ?? (format ? detectOfficeExtension(`x.${format}`) : null);
    const open = () => {
        if (!refid) return;
        openOfficePreviewModal({ title: name, extension: ext, source: { refId: refid } });
    };
    return (
        <span
            role="button"
            tabIndex={0}
            onClick={open}
            onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") {
                    e.preventDefault();
                    open();
                }
            }}
            title={refid ? `打开 ${name}` : name}
            className="mt-2 inline-flex min-w-[220px] max-w-full cursor-pointer items-start gap-2.5 rounded-lg border border-border bg-muted/40 px-3 py-2.5 text-left text-foreground transition-colors hover:bg-muted/70"
            data-testid="message-office-selection"
        >
            <OfficeFormatIcon ext={ext} className="mt-0.5 h-4 w-4 shrink-0 text-muted-foreground" />
            <span className="min-w-0">
                <span className="block truncate text-sm font-medium text-foreground">{name}</span>
                {location && <span className="block truncate text-xs text-muted-foreground">{location}</span>}
                {excerpt && <span className="mt-0.5 line-clamp-2 block text-xs text-muted-foreground">「{excerpt}」</span>}
            </span>
        </span>
    );
};

export default MessageOfficeSelection;
