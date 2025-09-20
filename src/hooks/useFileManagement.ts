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

    const getAttachmentType = useCallback((fileType: string) => {
        if (fileType.startsWith("image/")) {
            return AttachmentType.Image;
        } else if (fileType === "text/plain") {
            return AttachmentType.Text;
        } else {
            return AttachmentType.Text;
        }
    }, []);

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
                    (file) =>
                        file.type === "image/png" ||
                        file.type === "image/jpeg" ||
                        file.type === "image/gif" ||
                        file.type === "text/plain",
                );

                console.log("paste files", files);

                const filePromises = files.map(
                    (file) =>
                        new Promise<FileInfo>((resolve, reject) => {
                            const reader = new FileReader();
                            reader.onload = async (event) => {
                                const fileContent = event.target?.result;
                                if (fileContent) {
                                    let newFile: FileInfo = {
                                        id: -1,
                                        name: file.name,
                                        path: file.name,
                                        type: getAttachmentType(file.type),
                                        thumbnail:
                                            typeof fileContent === "string"
                                                ? fileContent
                                                : undefined,
                                    };

                                    try {
                                        console.log(
                                            "trigger add attachment api",
                                        );
                                        const res =
                                            await invoke<AddAttachmentResponse>(
                                                "add_attachment",
                                                {
                                                    fileContent: fileContent,
                                                    fileName: file.name,
                                                    attachmentType:
                                                        newFile.type,
                                                },
                                            );
                                        newFile.id = res.attachment_id;
                                    } catch (error) {
                                        toast.error("文件上传失败: " + error);
                                    }

                                    resolve(newFile);
                                } else {
                                    reject(
                                        new Error(
                                            "Failed to read file content",
                                        ),
                                    );
                                }
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
            }
        },
        [getAttachmentType, onFileSelect],
    );

    return {
        fileInfoList,
        clearFileInfoList,
        handleChooseFile,
        handleDeleteFile,
        handlePaste,
    };
};

export default useFileManagement;
