import { useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";
import {
    AddAttachmentResponse,
    AttachmentType,
    FileInfo,
} from "../data/Conversation";

type FileSelectCallback = (files: FileInfo[]) => void;

const useFileManagement = (onFileSelect?: FileSelectCallback) => {
    const [fileInfoList, setFileInfoList] = useState<Array<FileInfo> | null>(
        null,
    );

    const clearFileInfoList = useCallback(() => {
        setFileInfoList(null);
    }, []);

    const isSupportedFile = useCallback((file: File) => {
        return file.type.startsWith("image/") || file.type === "text/plain";
    }, []);

    const getAttachmentType = useCallback((fileType: string) => {
        if (fileType.startsWith("image/")) {
            return AttachmentType.Image;
        } else if (fileType === "text/plain") {
            return AttachmentType.Text;
        } else {
            return AttachmentType.Text;
        }
    }, []);

    const processFiles = useCallback(
        async (files: File[]) => {
            const filePromises = files.map(
                (file) =>
                    new Promise<FileInfo>((resolve, reject) => {
                        const reader = new FileReader();
                        reader.onload = async (event) => {
                            const fileContent = event.target?.result;
                            if (typeof fileContent !== "string") {
                                reject(new Error("Failed to read file content"));
                                return;
                            }

                            let newFile: FileInfo = {
                                id: -1,
                                name: file.name,
                                path: file.name,
                                type: getAttachmentType(file.type),
                                thumbnail:
                                    file.type.startsWith("image/") ? fileContent : undefined,
                            };

                            try {
                                const res = await invoke<AddAttachmentResponse>(
                                    "add_attachment",
                                    {
                                        fileContent,
                                        fileName: file.name,
                                        attachmentType: newFile.type,
                                    },
                                );
                                newFile.id = res.attachment_id;
                            } catch (error) {
                                toast.error("文件上传失败: " + error);
                            }

                            resolve(newFile);
                        };
                        reader.onerror = reject;

                        if (file.type.startsWith("image/")) {
                            reader.readAsDataURL(file);
                        } else {
                            reader.readAsText(file);
                        }
                    }),
            );

            try {
                const newFiles = await Promise.all(filePromises);
                setFileInfoList((prev) => [...(prev || []), ...newFiles]);
                if (onFileSelect) {
                    onFileSelect(newFiles);
                }
            } catch (error) {
                toast.error("文件处理失败: " + error);
            }
        },
        [getAttachmentType, onFileSelect],
    );

    const handleChooseFile = useCallback(async () => {
        console.log("trigger handleChooseFile");

        try {
            const selected = await open({
                multiple: true,
            });

            if (selected) {
                const paths = Array.isArray(selected) ? selected : [selected];
                const filePromises = paths.map(async (path) => {
                    const name =
                        path.split("\\").pop() || path.split("/").pop() || "";
                    const contents = await readFile(path);

                    let thumbnail = undefined;
                    let type = AttachmentType.Text;

                    if (name.match(/\.(jpg|jpeg|png|gif)$/i)) {
                        const blob = new Blob([contents]);
                        thumbnail = URL.createObjectURL(blob);
                        type = AttachmentType.Image;
                    }

                    const newFile: FileInfo = {
                        id: -1,
                        name,
                        path,
                        thumbnail,
                        type,
                    };

                    try {
                        const res = await invoke<AddAttachmentResponse>(
                            "add_attachment",
                            {
                                fileUrl: path,
                            },
                        );
                        newFile.id = res.attachment_id;
                    } catch (error) {
                        toast.error("文件上传失败: " + JSON.stringify(error));
                    }

                    return newFile;
                });

                const newFiles = await Promise.all(filePromises);
                setFileInfoList((prev) => [...(prev || []), ...newFiles]);
                if (onFileSelect) {
                    onFileSelect(newFiles);
                }
            }
        } catch (error) {
            toast.error("文件选择失败: " + error);
        }
    }, [onFileSelect]);

    const handleDeleteFile = useCallback((fileId: number) => {
        setFileInfoList((prevList) =>
            prevList ? prevList.filter((file) => file.id !== fileId) : null,
        );
    }, []);

    const handlePaste = useCallback(
        async (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
            console.log("trigger handlePaste", e);
            if (e.clipboardData.files.length > 0) {
                e.preventDefault(); // Prevent default paste behavior
                const files = Array.from(e.clipboardData.files).filter(
                    (file) => isSupportedFile(file),
                );

                console.log("paste files", files);
                if (files.length === 0) {
                    toast.error("暂不支持该文件类型");
                    return;
                }

                await processFiles(files);
            }
        },
        [isSupportedFile, processFiles],
    );

    const handleDropFiles = useCallback(
        (files: File[]) => {
            const supportedFiles = files.filter((file) => isSupportedFile(file));
            if (supportedFiles.length === 0) {
                toast.error("暂不支持该文件类型");
                return;
            }

            void processFiles(supportedFiles);
        },
        [isSupportedFile, processFiles],
    );

    return {
        fileInfoList,
        clearFileInfoList,
        handleChooseFile,
        handleDeleteFile,
        handlePaste,
        handleDropFiles,
    };
};

export default useFileManagement;
