import { describe, expect, it } from "vitest";
import {
    detectOfficeExtension,
    extractPreviewRelayToken,
    isLegacyOfficeExtension,
    isModernOfficeExtension,
    isOfficeFileName,
    officeFormatLabel,
    parseOfficeFileResponse,
    parseOfficePreviewError,
    viewerTargetOrigin,
} from "./officeFile";

describe("officeFile utilities", () => {
    it("should detect modern and legacy Office extensions accurately", () => {
        expect(detectOfficeExtension("季度报告.DOCX")).toBe("docx");
        expect(detectOfficeExtension("C:\\docs\\财务汇总.xlsx?v=1")).toBe("xlsx");
        expect(detectOfficeExtension("/tmp/slides.pptx#page=2")).toBe("pptx");
        expect(detectOfficeExtension("legacy.doc")).toBe("doc");
        expect(detectOfficeExtension("legacy.xls")).toBe("xls");
        expect(detectOfficeExtension("legacy.ppt")).toBe("ppt");
        expect(detectOfficeExtension("readme.md")).toBeNull();
        expect(detectOfficeExtension("")).toBeNull();

        expect(isModernOfficeExtension("docx")).toBe(true);
        expect(isModernOfficeExtension("doc")).toBe(false);
        expect(isLegacyOfficeExtension("doc")).toBe(true);
        expect(isOfficeFileName("演示.pptx")).toBe(true);
        expect(officeFormatLabel("xlsx")).toBe("Excel 表格 (.xlsx)");
    });

    it("should extract relay token from aipp-preview URLs", () => {
        expect(extractPreviewRelayToken("aipp-preview://localhost/9f72a1c0-1234")).toBe("9f72a1c0-1234");
        expect(extractPreviewRelayToken("http://aipp-preview.localhost/abc-def-123")).toBe("abc-def-123");
        expect(extractPreviewRelayToken("https://example.com/file.docx")).toBeNull();
    });

    it("should parse structured OFFICE_PREVIEW_ERROR wire strings into localized titles", () => {
        const enc = parseOfficePreviewError(
            "OFFICE_PREVIEW_ERROR:password_encrypted:该文件受密码保护，暂不支持应用内预览",
        );
        expect(enc.code).toBe("password_encrypted");
        expect(enc.title).toBe("文件受密码加密保护");
        expect(enc.message).toContain("密码保护");

        const corrupt = parseOfficePreviewError(
            new Error("OFFICE_PREVIEW_ERROR:corrupt_file:缺少核心部件 word/document.xml"),
        );
        expect(corrupt.code).toBe("corrupt_file");
        expect(corrupt.title).toBe("文件已损坏或结构异常");

        const large = parseOfficePreviewError(
            "OFFICE_PREVIEW_ERROR:file_too_large:文件大小（45.0 MiB）超过上限（32 MiB）",
        );
        expect(large.code).toBe("file_too_large");
        expect(large.title).toBe("文件大小超出预览限制");
    });
    it("should split the binary office file response into header and raw bytes", () => {
        const header = new TextEncoder().encode(
            JSON.stringify({ fileName: "a.xlsx", format: "xlsx", mime: "m", sizeBytes: 3, sha256: "h" }),
        );
        const raw = new Uint8Array(4 + header.length + 3);
        new DataView(raw.buffer).setUint32(0, header.length, true);
        raw.set(header, 4);
        raw.set([7, 8, 9], 4 + header.length);

        const payload = parseOfficeFileResponse(raw.buffer);
        expect(payload.fileName).toBe("a.xlsx");
        expect(payload.sha256).toBe("h");
        expect(Array.from(new Uint8Array(payload.bytes))).toEqual([7, 8, 9]);

        expect(() => parseOfficeFileResponse(raw.buffer.slice(0, raw.length - 1))).toThrow(/字节数/);
        expect(() => parseOfficeFileResponse(new ArrayBuffer(2))).toThrow(/OFFICE_PREVIEW_ERROR:read_error/);
    });

    it("should use a concrete postMessage origin except for opaque custom schemes", () => {
        expect(viewerTargetOrigin("http://aipp-office.localhost/index.html")).toBe("http://aipp-office.localhost");
        expect(viewerTargetOrigin("aipp-office://localhost/index.html")).toBe("*");
    });
});
