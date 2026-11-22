import { convertFileSrc } from "@tauri-apps/api/core";

export type ModernOfficeExtension = "docx" | "xlsx" | "pptx";
export type LegacyOfficeExtension = "doc" | "xls" | "ppt";
export type OfficeExtension = ModernOfficeExtension | LegacyOfficeExtension;

export interface OfficePreviewSource {
    /** office_file_ref 引用号：用户交给助手的文件或 AI 生成的副本。 */
    refId?: string;
    attachmentId?: number;
    relayToken?: string;
    localPath?: string;
}

export interface OfficePreviewTarget {
    title: string;
    source: OfficePreviewSource;
    extension?: OfficeExtension | null;
    /** 打开预览的会话；“让 AI 修改”把选区发到这个会话（缺省时发到当前窗口正在显示的会话）。 */
    conversationId?: number | null;
}

export interface ParsedOfficePreviewError {
    code: string;
    title: string;
    message: string;
}

export const OFFICE_PREVIEW_PLUGIN_CODE = "office-preview-plugin";
export const OPEN_OFFICE_PREVIEW_EVENT = "aipp:open-office-preview";

export function detectOfficeExtension(nameOrPath?: string | null): OfficeExtension | null {
    if (!nameOrPath) return null;
    const clean = nameOrPath.split("?")[0].split("#")[0].trim();
    const dot = clean.lastIndexOf(".");
    if (dot < 0) return null;
    const ext = clean.slice(dot + 1).toLowerCase();
    if (ext === "docx" || ext === "xlsx" || ext === "pptx" || ext === "doc" || ext === "xls" || ext === "ppt") {
        return ext;
    }
    return null;
}

export function isModernOfficeExtension(ext?: string | null): ext is ModernOfficeExtension {
    return ext === "docx" || ext === "xlsx" || ext === "pptx";
}

export function isLegacyOfficeExtension(ext?: string | null): ext is LegacyOfficeExtension {
    return ext === "doc" || ext === "xls" || ext === "ppt";
}

export function isOfficeFileName(nameOrPath?: string | null): boolean {
    return detectOfficeExtension(nameOrPath) !== null;
}

export function officeFormatLabel(ext?: string | null): string {
    switch (ext?.toLowerCase()) {
        case "docx":
            return "Word 文档 (.docx)";
        case "xlsx":
            return "Excel 表格 (.xlsx)";
        case "pptx":
            return "PowerPoint 演示文稿 (.pptx)";
        case "doc":
            return "旧版 Word 文档 (.doc)";
        case "xls":
            return "旧版 Excel 表格 (.xls)";
        case "ppt":
            return "旧版 PowerPoint (.ppt)";
        default:
            return "Office 文档";
    }
}

export function extractPreviewRelayToken(url?: string | null): string | null {
    if (!url) return null;
    const trimmed = url.trim();
    const m1 = /^aipp-preview:\/\/localhost\/([\w-]+)$/i.exec(trimmed);
    if (m1) return m1[1];
    const m2 = /^https?:\/\/aipp-preview\.localhost\/([\w-]+)$/i.exec(trimmed);
    if (m2) return m2[1];
    return null;
}

export function resolveOfficeViewerUrl(): string {
    try {
        return convertFileSrc("index.html", "aipp-office");
    } catch {
        return "aipp-office://localhost/index.html";
    }
}

export function parseOfficePreviewError(raw: unknown): ParsedOfficePreviewError {
    const str =
        raw instanceof Error
            ? raw.message
            : typeof raw === "string"
              ? raw
              : raw != null
                ? JSON.stringify(raw)
                : "未知预览错误";

    const prefix = "OFFICE_PREVIEW_ERROR:";
    const idx = str.indexOf(prefix);
    let code = "read_error";
    let message = str;
    if (idx >= 0) {
        const rest = str.slice(idx + prefix.length);
        const sep = rest.indexOf(":");
        if (sep > 0) {
            code = rest.slice(0, sep).trim();
            message = rest.slice(sep + 1).trim();
        }
    }

    const titleMap: Record<string, string> = {
        corrupt_file: "文件已损坏或结构异常",
        password_encrypted: "文件受密码加密保护",
        unsupported_legacy_format: "暂不支持旧版二进制 Office 格式",
        unsupported_format: "不支持的文件格式",
        file_too_large: "文件大小超出预览限制",
        file_not_found: "找不到目标文件",
        render_timeout: "文档渲染超时",
        permission_denied: "文件未获授权",
        read_error: "文件读取失败",
        render_error: "文档渲染失败",
        stale_selection: "文件已被修改",
        target_exists: "目标文件已存在",
        protected_source: "不能覆盖原文件",
        target_dir_missing: "保存位置不存在",
        invalid_target: "保存位置无效",
        disk_full: "磁盘空间不足",
        write_failed: "保存失败",
        invalid_request: "请求无效",
    };

    return {
        code,
        title: titleMap[code] ?? "文档预览失败",
        message: message || "未提供详细错误原因",
    };
}

export interface OfficeFilePayload {
    fileName: string;
    format: ModernOfficeExtension;
    mime: string;
    sizeBytes: number;
    sha256: string;
    copyOriginToken?: string;
    bytes: ArrayBuffer;
}

/**
 * 解析 `read_authorized_office_file` 的二进制响应：
 * `[u32 LE 头长度][UTF-8 JSON 头][文件字节]`。字节直接切片，不经 base64。
 */
export function parseOfficeFileResponse(raw: ArrayBuffer | Uint8Array | number[]): OfficeFilePayload {
    const buffer: ArrayBuffer =
        raw instanceof ArrayBuffer ? raw : new Uint8Array(raw).buffer;
    if (buffer.byteLength < 4) {
        throw new Error("OFFICE_PREVIEW_ERROR:read_error:文件读取响应不完整（缺少头部）");
    }
    const headerLen = new DataView(buffer).getUint32(0, true);
    if (4 + headerLen > buffer.byteLength) {
        throw new Error("OFFICE_PREVIEW_ERROR:read_error:文件读取响应头部长度越界");
    }
    const header = JSON.parse(new TextDecoder().decode(new Uint8Array(buffer, 4, headerLen))) as Omit<
        OfficeFilePayload,
        "bytes"
    >;
    const bytes = buffer.slice(4 + headerLen);
    if (bytes.byteLength !== header.sizeBytes) {
        throw new Error(
            `OFFICE_PREVIEW_ERROR:read_error:文件字节数与头部声明不一致（${bytes.byteLength} != ${header.sizeBytes}）`,
        );
    }
    return { ...header, bytes };
}

/** 二进制请求：`[u32 LE 头长度][UTF-8 JSON 头][文件字节]`，与读取响应同格式，不经 base64。 */
export function encodeFramedRequest(header: Record<string, unknown>, bytes: ArrayBuffer | Uint8Array): Uint8Array {
    const headerBytes = new TextEncoder().encode(JSON.stringify(header));
    const body = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
    const out = new Uint8Array(4 + headerBytes.byteLength + body.byteLength);
    new DataView(out.buffer).setUint32(0, headerBytes.byteLength, true);
    out.set(headerBytes, 4);
    out.set(body, 4 + headerBytes.byteLength);
    return out;
}

export function officeSourceKey(source: OfficePreviewSource): string {
    return `${source.refId ?? ""}|${source.attachmentId ?? ""}|${source.relayToken ?? ""}|${source.localPath ?? ""}`;
}

/** postMessage 的目标 origin；自定义协议（如 macOS 的 aipp-office://）origin 为 "null"，只能用 "*"。 */
export function viewerTargetOrigin(viewerUrl: string): string {
    try {
        const origin = new URL(viewerUrl).origin;
        return origin && origin !== "null" ? origin : "*";
    } catch {
        return "*";
    }
}

export function openOfficePreviewModal(target: OfficePreviewTarget): void {
    window.dispatchEvent(new CustomEvent<OfficePreviewTarget>(OPEN_OFFICE_PREVIEW_EVENT, { detail: target }));
}
