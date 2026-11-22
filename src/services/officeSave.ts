/**
 * Office 副本另存（#331）：默认另存为新文件，不覆盖原文件。
 * 桌面：系统“另存为”对话框 → Rust 原子写入（目标已存在时返回 target_exists，由界面二次确认后才覆盖）。
 * Android：系统文档创建（SAF），不会覆盖同名文件；“已保存到本地”与“调起分享”分开。
 */
import { invoke } from "@tauri-apps/api/core";
import { save } from "@tauri-apps/plugin-dialog";
import {
    encodeFramedRequest,
    parseOfficeFileResponse,
    parseOfficePreviewError,
    type ModernOfficeExtension,
    type OfficePreviewSource,
    type ParsedOfficePreviewError,
} from "@/utils/officeFile";
import { exportMime, isAndroid, mobileFileAction, type MobileFileResult } from "@/services/mobileFileService";

export interface OfficeSaveResult {
    path: string;
    fileName: string;
    sizeBytes: number;
    format: string;
}

export type OfficeCopyPurpose = "copy" | "output";

export interface OfficeSaveRequest {
    /** 已授权的来源：用于默认路径、保护原文件，以及 AI 副本直接复制。 */
    source: OfficePreviewSource;
    fileName: string;
    format: ModernOfficeExtension;
    /** copy：手动编辑后的内容（需 bytes）；output：AI 生成的副本文件本身。 */
    purpose: OfficeCopyPurpose;
    copyOriginToken?: string;
    bytes?: ArrayBuffer;
}

/** Android 写入上限，与插件导出上限一致。 */
export const MOBILE_SAVE_LIMIT_BYTES = 32 * 1024 * 1024;

export const OFFICE_FORMAT_SAVE_NOTES: Record<ModernOfficeExtension, string> = {
    docx: "应用内显示用开源字体代替宋体、微软雅黑等字体，保存时保留原字体名；宏、ActiveX 等内容无法编辑。",
    xlsx: "数据透视表、宏（VBA）与外部链接无法编辑；复杂组合图在应用内的显示可能与 Excel 有差异，图表数据原样保留。",
    pptx: "动画与切换在应用内不播放，保存时原样保留；SmartArt、三维效果与音视频无法编辑。",
};

export class OfficeSaveError extends Error {
    readonly code: string;
    readonly title: string;
    constructor(parsed: ParsedOfficePreviewError) {
        super(parsed.message);
        this.code = parsed.code;
        this.title = parsed.title;
    }
}

function toSaveError(err: unknown): OfficeSaveError {
    return err instanceof OfficeSaveError ? err : new OfficeSaveError(parseOfficePreviewError(err));
}

export async function suggestOfficeCopyPath(source: OfficePreviewSource, purpose: OfficeCopyPurpose): Promise<string> {
    return invoke<string>("suggest_office_copy_path", { source, purpose });
}

/** 桌面“另存为”对话框；取消时返回 null。 */
export async function pickOfficeSaveTarget(request: OfficeSaveRequest): Promise<string | null> {
    let defaultPath: string | undefined;
    try {
        defaultPath = await suggestOfficeCopyPath(request.source, request.purpose);
    } catch (err) {
        console.warn("[officeSave] suggest_office_copy_path failed:", err);
    }
    const label = request.format === "docx" ? "Word 文档" : request.format === "xlsx" ? "Excel 表格" : "PowerPoint 演示文稿";
    const path = await save({ defaultPath, filters: [{ name: label, extensions: [request.format] }] });
    return path ?? null;
}

/** 桌面写入；目标已存在且未确认覆盖时抛出 code=target_exists。 */
export async function writeOfficeCopy(request: OfficeSaveRequest, targetPath: string, overwrite: boolean): Promise<OfficeSaveResult> {
    try {
        if (request.purpose === "copy") {
            if (!request.bytes) throw new Error("OFFICE_PREVIEW_ERROR:invalid_request:没有可保存的文档内容");
            const body = encodeFramedRequest({
                source: request.source, copyOriginToken: request.copyOriginToken, targetPath, overwrite,
            }, request.bytes);
            return await invoke<OfficeSaveResult>("office_write_copy", body);
        }
        return await invoke<OfficeSaveResult>("office_save_copy", { source: request.source, targetPath, overwrite });
    } catch (err) {
        throw toSaveError(err);
    }
}

async function bytesForMobile(request: OfficeSaveRequest): Promise<Uint8Array> {
    if (request.bytes) return new Uint8Array(request.bytes);
    const raw = await invoke<ArrayBuffer>("read_authorized_office_file", { source: request.source });
    return new Uint8Array(parseOfficeFileResponse(raw).bytes);
}

function toBase64(bytes: Uint8Array): string {
    let binary = "";
    for (let offset = 0; offset < bytes.length; offset += 8192) {
        binary += String.fromCharCode(...bytes.subarray(offset, offset + 8192));
    }
    return btoa(binary);
}

/** Android：系统创建文档（不覆盖同名文件）。只有 status=saved 才算保存成功。 */
export async function saveOfficeOnAndroid(request: OfficeSaveRequest, name: string): Promise<MobileFileResult> {
    try {
        const bytes = await bytesForMobile(request);
        if (bytes.byteLength > MOBILE_SAVE_LIMIT_BYTES) {
            throw new Error(
                `OFFICE_PREVIEW_ERROR:file_too_large:文件 ${(bytes.byteLength / 1048576).toFixed(1)} MiB，超过移动端保存上限 32 MiB，请在电脑上另存`,
            );
        }
        return await mobileFileAction("save", { data: toBase64(bytes), name, mime: exportMime(name) });
    } catch (err) {
        throw toSaveError(err);
    }
}

/** Android：调起系统分享。只代表已打开分享面板，不代表对方已收到或文件已保存。 */
export async function shareOfficeOnAndroid(request: OfficeSaveRequest, name: string): Promise<MobileFileResult> {
    try {
        const bytes = await bytesForMobile(request);
        return await mobileFileAction("share", { data: toBase64(bytes), name, mime: exportMime(name) });
    } catch (err) {
        throw toSaveError(err);
    }
}

export { isAndroid };
