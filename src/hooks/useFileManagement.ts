import { useState, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
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

    const getOfficeAttachmentTypeByName = useCallback((nameOrMime: string): AttachmentType | null => {
        const lower = nameOrMime.toLowerCase();
        if (lower.endsWith(".docx") || lower.includes("wordprocessingml.document")) {
            return AttachmentType.Word;
        }
        if (lower.endsWith(".xlsx") || lower.includes("spreadsheetml.sheet")) {
            return AttachmentType.Excel;
        }
        if (lower.endsWith(".pptx") || lower.includes("presentationml.presentation")) {
            return AttachmentType.PowerPoint;
        }
        return null;
    }, []);

    const getAttachmentType = useCallback((mime: string, name: string) =>
        getOfficeAttachmentTypeByName(name) ?? getOfficeAttachmentTypeByName(mime) ??
        (mime.startsWith("image/") ? AttachmentType.Image : mime === "application/pdf" ? AttachmentType.PDF :
            mime.startsWith("text/") ? AttachmentType.Text : AttachmentType.File), [getOfficeAttachmentTypeByName]);

    const processFiles = useCallback(
        async (files: File[]) => {
            if (await isAndroid() && (files.length > 10 || files.some(file => file.size > 20 * 1024 * 1024)
                || files.reduce((total, file) => total + file.size, 0) > 50 * 1024 * 1024)) {
                toast.error("附件超过移动端限制：最多 10 个、单文件 20 MiB、总计 50 MiB");
                return;
            }
            const filePromises = files.map(async (file): Promise<FileInfo> => {
                const path = (file as File & { path?: string }).path;
                let dataUrl: string | undefined;
                if (!path) {
                    dataUrl = await new Promise<string>((resolve, reject) => {
                        const reader = new FileReader();
                        reader.onerror = () => reject(reader.error);
                        reader.onload = () => resolve(String(reader.result));
                        reader.readAsDataURL(file);
                    });
                }
                const res = await invoke<AddAttachmentResponse>("add_attachment",
                    path ? { fileUrl: path } : { fileName: file.name, fileDataUrl: dataUrl });
                if (res.attachment_id <= 0) throw new Error("附件登记未返回有效 ID");
                return { id: res.attachment_id, name: file.name, path: res.path ?? path ?? file.name,
                    type: res.attachment_type ?? getAttachmentType(file.type, file.name),
                    thumbnail: res.thumbnail ?? (file.type.startsWith("image/") ? dataUrl : undefined) };
            });

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

    const addLocalPaths = useCallback(async (paths: string[]) => {
        const filePromises = paths.map(async (path) => {
            const name = path.split(/[\\/]/).pop() || path;

            const result = await invoke<AddAttachmentResponse>("add_attachment", { fileUrl: path });
            if (result.attachment_id <= 0) throw new Error("附件登记未返回有效 ID");
            return {
                id: result.attachment_id,
                name,
                path,
                thumbnail: result.thumbnail,
                type: result.attachment_type ?? getAttachmentType("", name),
            };
        });

        const results = await Promise.allSettled(filePromises);
        const newFiles = results.flatMap((result) => result.status === "fulfilled" ? [result.value] : []);
        results.forEach((result) => { if (result.status === "rejected") toast.error("文件上传失败: " + fileErrorMessage(result.reason)); });
        if (newFiles.length) editedSinceRestore.current = true;
        setFileInfoList((prev) => [...(prev || []), ...newFiles]);
        if (onFileSelect) {
            onFileSelect(newFiles);
        }
    }, [getAttachmentType, onFileSelect]);

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
                        newFiles.push({
                            id: res.attachment_id,
                            name: file.name,
                            path: file.path,
                            type: res.attachment_type ?? getAttachmentType(file.mime, file.name),
                            thumbnail: file.thumbnail,
                        });
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

            if (selected) await addLocalPaths(Array.isArray(selected) ? selected : [selected]);
        } catch (error) {
            toast.error("文件选择失败: " + fileErrorMessage(error));
        }
    }, [onFileSelect, getAttachmentType, addLocalPaths]);

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
                const files = Array.from(e.clipboardData.files);
                await processFiles(files);
            }
        },
        [processFiles],
    );

    const handleDropFiles = useCallback(
        (files: File[]) => {
            void processFiles(files);
        },
        [processFiles],
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
        addLocalPaths,
    };
};

export default useFileManagement;
