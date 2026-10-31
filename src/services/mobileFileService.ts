import { invoke } from "@tauri-apps/api/core";

export interface ImportedMobileFile {
    path: string;
    name: string;
    mime: string;
    size: number;
    thumbnail?: string;
}
export interface MobileFileResult {
    status: "cancelled" | "selected" | "saved" | "share_launched" | "opened" | "print_launched" | "copied";
    files?: ImportedMobileFile[];
    errors?: string[];
    uri?: string;
}
export const isAndroid = async (): Promise<boolean> => (await invoke<string>("get_platform")) === "android";
export const mobileFileAction = (action: string, payload: Record<string, unknown> = {}): Promise<MobileFileResult> =>
    invoke("mobile_file_action", { action, payload });

export function fileErrorMessage(error: unknown): string {
    if (error instanceof Error) return error.message;
    if (typeof error === "string") return error;
    // AppError is a tagged Rust enum, e.g. { IoError: "disk full" }.
    if (error != null) {
        try { return JSON.stringify(error) ?? String(error); } catch { return String(error); }
    }
    return "文件操作失败，但调用层未提供错误详情";
}

export const exportMime = (name: string): string => {
    const extension = name.split(".").pop()?.toLowerCase();
    const types: Record<string, string> = { md: "text/markdown", docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", png: "image/png", pdf: "application/pdf", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp" };
    if (!extension || !types[extension]) throw new Error(`不支持的导出格式：${name}`);
    return types[extension];
};

export function encodeMobileExport(content: Uint8Array): string {
    if (content.byteLength > 32 * 1024 * 1024) throw new Error("导出文件超过移动端 32 MiB 限制，请减少消息数量");
    let binary = "";
    for (let offset = 0; offset < content.length; offset += 8192) {
        binary += String.fromCharCode(...content.subarray(offset, offset + 8192));
    }
    return btoa(binary);
}

export async function outputMobileFile(action: "save" | "share" | "open", content: Uint8Array, name: string): Promise<MobileFileResult> {
    return mobileFileAction(action, { data: encodeMobileExport(content), name, mime: exportMime(name) });
}
