import { useEffect, useState } from "react";
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import type { LocalFileLink } from "@/react-markdown/localFileLink";
import DelimitedTable from "./DelimitedTable";

type FileTarget = Extract<LocalFileLink, { kind: "table" | "image" | "audio" | "video" | "pdf" }>;
interface FileInfo {
    token: string;
    size: number;
}

export default function ChatBinaryFilePreview({ file, conversationId }: { file: FileTarget; conversationId?: number }) {
    const [info, setInfo] = useState<FileInfo>(),
        [error, setError] = useState("");
    useEffect(() => {
        let active = true,
            token: string | undefined;
        const close = (id: string) => void invoke("close_chat_preview_file", { token: id }).catch(console.error);
        setError("");
        setInfo(undefined);
        void invoke<FileInfo>("open_chat_preview_file", { path: file.path, conversationId }).then(
            (result) => {
                if (!active) {
                    close(result.token);
                    return;
                }
                token = result.token;
                setInfo(result);
            },
            (e) => {
                if (active) setError(String(e));
            }
        );
        return () => {
            active = false;
            if (token) close(token);
        };
    }, [file.path, conversationId]);
    if (error)
        return (
            <p role="alert" className="p-4 text-destructive">
                {error}
            </p>
        );
    if (!info) return <p className="p-4">正在读取文件…</p>;
    const url = convertFileSrc(info.token, "aipp-file");
    if (file.kind === "table")
        return <DelimitedTable url={url} size={info.size} delimiter={file.extension === "tsv" ? "\t" : ","} />;
    if (file.kind === "pdf") return <iframe src={url} title={file.title} className="min-h-0 w-full flex-1 border-0" />;
    const failed = () => setError(`无法显示 ${file.title}，文件可能损坏或当前系统不支持此编码。`);
    return (
        <div className="flex min-h-0 flex-1 items-center justify-center overflow-auto bg-muted/25 p-4">
            {file.kind === "image" && (
                <img src={url} alt={file.title} onError={failed} className="max-h-full max-w-full object-contain" />
            )}
            {file.kind === "video" && (
                <video controls src={url} onError={failed} aria-label={file.title} className="max-h-full max-w-full" />
            )}
            {file.kind === "audio" && (
                <audio controls src={url} onError={failed} aria-label={file.title} className="w-full max-w-xl" />
            )}
        </div>
    );
}
