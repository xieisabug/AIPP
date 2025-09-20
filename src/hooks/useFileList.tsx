import React from 'react';
import { FileInfo, AttachmentType } from '../data/Conversation';
import Text from '../assets/text.svg?react';
import IconButton from '@/components/IconButton';
import Delete from "../assets/delete.svg?react";

interface FileListRendererProps {
  fileInfo: FileInfo;
  onOpen?: (fileId: number) => void;
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
          alt="缩略图"
          className="input-area-img"
        />
      );
    case AttachmentType.Text:
      return [
        <Text fill="black" />,
        <span title={fileInfo.name}>
          {fileInfo.name}
        </span>,
      ];
    case AttachmentType.PDF:
      return (
        <span title={fileInfo.name}>
          {fileInfo.name} (PDF)
        </span>
      );
    case AttachmentType.Word:
      return (
        <span title={fileInfo.name}>
          {fileInfo.name} (Word)
        </span>
      );
    case AttachmentType.PowerPoint:
      return (
        <span title={fileInfo.name}>
          {fileInfo.name} (PowerPoint)
        </span>
      );
    case AttachmentType.Excel:
      return (
        <span title={fileInfo.name}>
          {fileInfo.name} (Excel)
        </span>
      );
    default:
      return (
        <span title={fileInfo.name}>
          {fileInfo.name}
        </span>
      );
  }
};

export const useFileList = (fileInfoList: FileInfo[] | null, onDelete: (fileId: number) => void, onOpen: (fileId: number) => void) => {
  const renderFiles = React.useCallback(() => {
    return fileInfoList?.map((fileInfo) => (
      <div
        key={fileInfo.name + fileInfo.id}
        className={
          fileInfo.type === AttachmentType.Image
            ? "input-area-img-wrapper"
            : "input-area-text-wrapper"
        }
      >
        <FileListRenderer fileInfo={fileInfo} onOpen={onOpen} />

        <IconButton
          border
          icon={<Delete fill="black" />}
          className="input-area-img-delete-button"
          onClick={() => {
            fileInfo.id && onDelete(fileInfo.id);
          }}
        />
      </div>
    ));
  }, [fileInfoList, onDelete]);

  return { renderFiles };
};
