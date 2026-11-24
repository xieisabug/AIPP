import React from 'react';
import {
  File as FileIcon,
  FileSpreadsheet,
  FileText,
  Image as ImageIcon,
  Presentation,
  Sparkles,
  X,
  type LucideIcon,
} from "lucide-react";
import { FileInfo, AttachmentType } from '../data/Conversation';

interface FileListRendererProps {
  fileInfo: FileInfo;
  onOpen?: (fileId: number) => void;
}

// 中日韩/全角字符约占两个西文字符宽，按显示宽度截取，避免中文名被 CSS 再截一次出现双省略号
const WIDE_CHAR = /[\u1100-\u115f\u2e80-\ua4cf\uac00-\ud7a3\uf900-\ufaff\ufe30-\ufe4f\uff00-\uff60\uffe0-\uffe6]/;

function charWidth(ch: string): number {
  return WIDE_CHAR.test(ch) || (ch.codePointAt(0) ?? 0) >= 0x1f000 ? 2 : 1;
}

function takeWidth(chars: string[], budget: number): number {
  let used = 0;
  let count = 0;
  for (const ch of chars) {
    const w = charWidth(ch);
    if (used + w > budget) break;
    used += w;
    count += 1;
  }
  return count;
}

/** 保留开头和结尾（同前缀的副本靠结尾区分）+ 扩展名；head/tail 以西文字符宽度计。 */
export function middleEllipsis(name: string, head = 8, tail = 10): string {
  const dot = name.lastIndexOf(".");
  const extension = dot > 0 && name.length - dot <= 8 ? name.slice(dot) : "";
  const stem = extension ? name.slice(0, name.length - extension.length) : name;
  const chars = Array.from(stem);
  const total = chars.reduce((sum, ch) => sum + charWidth(ch), 0);
  if (total <= head + tail + 1) return name;
  const headCount = takeWidth(chars, head);
  const tailCount = takeWidth([...chars].reverse(), tail);
  return `${chars.slice(0, headCount).join("")}…${chars.slice(chars.length - tailCount).join("")}${extension}`;
}

function fileExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || name.length - dot > 8) return "";
  return name.slice(dot + 1).toUpperCase();
}

function attachmentTypeLabel(fileInfo: FileInfo): string {
  switch (fileInfo.type) {
    case AttachmentType.Image:
      return "IMG";
    case AttachmentType.Text:
      return "TXT";
    case AttachmentType.PDF:
      return "PDF";
    case AttachmentType.Word:
      return "DOC";
    case AttachmentType.PowerPoint:
      return "PPT";
    case AttachmentType.Excel:
      return "XLS";
    case AttachmentType.Skill:
      return "SKILL";
    default:
      // 通用文件用扩展名比笼统的 FILE 更有信息量
      return fileExtension(fileInfo.name) || "FILE";
  }
}

function attachmentIcon(type: AttachmentType): LucideIcon {
  switch (type) {
    case AttachmentType.Image:
      return ImageIcon;
    case AttachmentType.Text:
    case AttachmentType.PDF:
    case AttachmentType.Word:
      return FileText;
    case AttachmentType.Excel:
      return FileSpreadsheet;
    case AttachmentType.PowerPoint:
      return Presentation;
    case AttachmentType.Skill:
      return Sparkles;
    default:
      return FileIcon;
  }
}

function AttachmentName({ name }: { name: string }) {
  const [expanded, setExpanded] = React.useState(false);
  const short = middleEllipsis(name);
  const truncated = short !== name;
  if (!truncated) {
    return <span className="input-area-attachment-name" title={name}>{name}</span>;
  }
  return (
    <button
      type="button"
      className="input-area-attachment-name"
      title={name}
      aria-expanded={expanded}
      onClick={(event) => {
        event.stopPropagation();
        setExpanded((open) => !open);
      }}
    >
      {expanded ? name : short}
    </button>
  );
}

const FileListRenderer: React.FC<FileListRendererProps> = ({ fileInfo, onOpen }) => {
  const meta = (
    <span className="input-area-attachment-meta">
      <AttachmentName name={fileInfo.name} />
      <span className="input-area-attachment-type" aria-hidden>
        {attachmentTypeLabel(fileInfo)}
      </span>
    </span>
  );
  if (fileInfo.type === AttachmentType.Image) {
    return (
      <>
        <button
          type="button"
          className="input-area-image-preview-button"
          aria-label={`预览 ${fileInfo.name || "图片"}`}
          onClick={(event) => {
            event.stopPropagation();
            onOpen?.(fileInfo.id);
          }}
        >
          <img
            src={fileInfo.thumbnail}
            alt={fileInfo.name || "缩略图"}
            className="input-area-img"
          />
        </button>
        {meta}
      </>
    );
  }
  const Icon = attachmentIcon(fileInfo.type);
  return (
    <>
      <span className="input-area-attachment-icon" aria-hidden>
        <Icon size={16} strokeWidth={1.75} />
      </span>
      {meta}
    </>
  );
};

export const useFileList = (
  fileInfoList: FileInfo[] | null,
  onDelete: (fileId: number) => void,
  onOpen: (fileId: number) => void,
) => {
  const renderFiles = React.useCallback(() => {
    return fileInfoList?.map((fileInfo) => (
      <div
        key={fileInfo.name + fileInfo.id}
        className={
          fileInfo.type === AttachmentType.Image
            ? "input-area-img-wrapper"
            : "input-area-text-wrapper"
        }
        data-aipp-slot="chat-input-attachment-item"
        data-attachment-type={AttachmentType[fileInfo.type] ?? fileInfo.type}
        data-attachment-name={fileInfo.name}
      >
        <div className="input-area-attachment-preview">
          <FileListRenderer fileInfo={fileInfo} onOpen={onOpen} />
        </div>

        <button
          type="button"
          className="input-area-img-delete-button"
          title={`移除 ${fileInfo.name}`}
          aria-label={`移除 ${fileInfo.name}`}
          data-aipp-slot="chat-input-attachment-remove"
          onClick={(event) => {
            event.stopPropagation();
            fileInfo.id && onDelete(fileInfo.id);
          }}
        >
          <X size={12} strokeWidth={2.5} />
        </button>
      </div>
    ));
  }, [fileInfoList, onDelete, onOpen]);

  return { renderFiles };
};
