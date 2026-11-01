import React from 'react';
import { FileInfo, AttachmentType } from '../data/Conversation';
import Text from '../assets/text.svg?react';
import IconButton from '@/components/IconButton';
import { Trash2 } from "lucide-react";

interface FileListRendererProps {
  fileInfo: FileInfo;
  onOpen?: (fileId: number) => void;
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

const FileListRenderer: React.FC<FileListRendererProps> = ({ fileInfo, onOpen }) => {
  switch (fileInfo.type) {
    case AttachmentType.Image:
      return (
        <img
          onClick={() => {
            onOpen && onOpen(fileInfo.id);
          }}
          src={fileInfo.thumbnail}
          alt={fileInfo.name || "缩略图"}
          className="input-area-img"
        />
      );
    case AttachmentType.Text:
      return (
        <>
          <Text className="input-area-attachment-icon" fill="currentColor" />
          <span className="input-area-attachment-name" title={fileInfo.name}>
            {fileInfo.name}
          </span>
          <span className="input-area-attachment-type" aria-hidden>
            {attachmentTypeLabel(fileInfo.type)}
          </span>
        </>
      );
    case AttachmentType.PDF:
    case AttachmentType.Word:
    case AttachmentType.PowerPoint:
    case AttachmentType.Excel:
    case AttachmentType.Skill:
    default:
      return (
        <>
          <span className="input-area-attachment-name" title={fileInfo.name}>
            {fileInfo.name}
          </span>
          <span className="input-area-attachment-type" aria-hidden>
            {attachmentTypeLabel(fileInfo.type)}
          </span>
        </>
      );
  }
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
