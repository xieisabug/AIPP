import { useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";
import { isAndroid, mobileFileAction, fileErrorMessage } from "@/services/mobileFileService";
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

    const editedSinceRestore = useRef(false);

    const clearFileInfoList = useCallback(() => {
        editedSinceRestore.current = true;
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
            if (await isAndroid() && (files.length > 10 || files.some(file => file.size > 20 * 1024 * 1024)
                || files.reduce((total, file) => total + file.size, 0) > 50 * 1024 * 1024)) {
                toast.error("附件超过移动端限制：最多 10 个、单文件 20 MiB、总计 50 MiB");
                return;
            }
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
                                if (newFile.id <= 0) throw new Error("附件登记未返回有效 ID");
                            } catch (error) {
                                reject(error);
                                return;
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
                const results = await Promise.allSettled(filePromises);
                const newFiles = results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
                results.forEach((result) => { if (result.status === "rejected") toast.error("文件上传失败: " + fileErrorMessage(result.reason)); });
                if (newFiles.length) editedSinceRestore.current = true;
                setFileInfoList((prev) => [...(prev || []), ...newFiles]);
                if (onFileSelect) {
                    onFileSelect(newFiles);
                }
            } catch (error) {
                toast.error("文件处理失败: " + fileErrorMessage(error));
            }
        },
        [getAttachmentType, onFileSelect],
    );

    const handleChooseFile = useCallback(async () => {
        console.log("trigger handleChooseFile");

        try {
            if (await isAndroid()) {
                const result = await mobileFileAction("pick");
                if (result.status === "cancelled") return;
                result.errors?.forEach((error) => toast.error(`文件导入失败: ${error}`));
                const newFiles: FileInfo[] = [];
                for (const file of result.files ?? []) {
                    try {
                        const res = await invoke<AddAttachmentResponse>("add_attachment", { fileUrl: file.path });
                        if (res.attachment_id <= 0) throw new Error("附件登记未返回有效 ID");
                        newFiles.push({ id: res.attachment_id, name: file.name, path: file.path,
                            type: file.mime.startsWith("image/") ? AttachmentType.Image : AttachmentType.Text,
                            thumbnail: file.thumbnail });
                    } catch (error) {
                        toast.error(`附件 ${file.name} 登记失败: ${fileErrorMessage(error)}`);
                    }
                }
                if (newFiles.length) editedSinceRestore.current = true;
                setFileInfoList((prev) => [...(prev ?? []), ...newFiles]);
                if (newFiles.length) onFileSelect?.(newFiles);
                return;
            }
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
                        if (newFile.id <= 0) throw new Error("附件登记未返回有效 ID");
                    } catch (error) {
                        if (thumbnail) URL.revokeObjectURL(thumbnail);
                        throw error;
                    }

                    return newFile;
                });

                const results = await Promise.allSettled(filePromises);
                const newFiles = results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
                results.forEach((result) => { if (result.status === "rejected") toast.error("文件上传失败: " + fileErrorMessage(result.reason)); });
                if (newFiles.length) editedSinceRestore.current = true;
                setFileInfoList((prev) => [...(prev || []), ...newFiles]);
                if (onFileSelect) {
                    onFileSelect(newFiles);
                }
            }
        } catch (error) {
            toast.error("文件选择失败: " + fileErrorMessage(error));
        }
    }, [onFileSelect]);

    const handleDeleteFile = useCallback((fileId: number) => {
        editedSinceRestore.current = true;
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

    const restoreFiles = useCallback((files: FileInfo[]) => {
        editedSinceRestore.current = false;
        setFileInfoList(files.length ? files : null);
    }, []);

    // Draft reads can resolve after the picker, paste or removal action. A newer
    // user selection (even an empty one) wins over the stored attachment list.
    const restoreDraftFiles = useCallback((files: FileInfo[]) => {
        if (!editedSinceRestore.current) setFileInfoList(files.length ? files : null);
    }, []);

    return {
        fileInfoList,
        clearFileInfoList,
        restoreFiles,
        restoreDraftFiles,
        handleChooseFile,
        handleDeleteFile,
        handlePaste,
        handleDropFiles,
    };
};

export default useFileManagement;
