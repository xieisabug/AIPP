import React from 'react';
import { FileInfo, AttachmentType } from '../data/Conversation';
import Text from '../assets/text.svg?react';
import IconButton from '@/components/IconButton';
import { Trash2 } from "lucide-react";

interface FileListRendererProps {
  fileInfo: FileInfo;
  onOpen?: (fileId: number) => void;
}

export function middleEllipsis(name: string, head = 10, tail = 12): string {
  const dot = name.lastIndexOf(".");
  const extension = dot > 0 && name.length - dot <= 8 ? name.slice(dot) : "";
  const stem = extension ? name.slice(0, name.length - extension.length) : name;
  if (stem.length <= head + tail + 1) return name;
  return `${stem.slice(0, head)}…${stem.slice(stem.length - tail)}${extension}`;
}

function attachmentTypeLabel(type: AttachmentType): string {
  switch (type) {
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
      return "FILE";
  }
}

function AttachmentName({ name }: { name: string }) {
  const [expanded, setExpanded] = React.useState(false);
  const short = middleEllipsis(name);
  const truncated = short !== name;
  if (!truncated) {
    return <span className="input-area-attachment-name">{name}</span>;
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
  const name = <AttachmentName name={fileInfo.name} />;
  const type = (
    <span className="input-area-attachment-type" aria-hidden>
      {attachmentTypeLabel(fileInfo.type)}
    </span>
  );
  if (fileInfo.type === AttachmentType.Image) {
    return (
      <>
        <img
          onClick={() => {
            onOpen && onOpen(fileInfo.id);
          }}
          src={fileInfo.thumbnail}
          alt={fileInfo.name || "缩略图"}
          className="input-area-img"
        />
        {name}
        {type}
      </>
    );
  }
  return (
    <>
      {fileInfo.type === AttachmentType.Text ? <Text className="input-area-attachment-icon" fill="currentColor" /> : null}
      {name}
      {type}
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

        <IconButton
          border
          icon={<Trash2 size={16} className="text-icon" />}
          className="input-area-img-delete-button"
          title={`移除 ${fileInfo.name}`}
          dataAippSlot="chat-input-attachment-remove"
          onClick={(event) => {
            event.stopPropagation();
            fileInfo.id && onDelete(fileInfo.id);
          }}
        />
      </div>
    ));
  }, [fileInfoList, onDelete, onOpen]);

  return { renderFiles };
};
