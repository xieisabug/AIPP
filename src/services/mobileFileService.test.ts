import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { encodeMobileExport, exportMime, outputMobileFile, fileErrorMessage } from "./mobileFileService";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

describe("mobile file export", () => {
    beforeEach(() => vi.clearAllMocks());

    it("should preserve tagged Rust errors when the backend returns AppError", () => {
        expect(fileErrorMessage({ IoError: "disk full" })).toContain("disk full");
        expect(fileErrorMessage(new Error("provider denied"))).toBe("provider denied");
    });

    it("should map supported extensions case insensitively and reject unsupported formats", () => {
        expect(exportMime("report.DOCX")).toBe("application/vnd.openxmlformats-officedocument.wordprocessingml.document");
        expect(exportMime("image.PNG")).toBe("image/png");
        expect(exportMime("report.md")).toBe("text/markdown");
        expect(exportMime("report.pdf")).toBe("application/pdf");
        expect(() => exportMime("report.exe")).toThrow("不支持的导出格式");
    });

    it("should preserve bytes and pass MIME to native save", async () => {
        vi.mocked(invoke).mockResolvedValue({ status: "cancelled" });
        await expect(outputMobileFile("save", new Uint8Array([0, 127, 128, 255]), "a.png")).resolves.toEqual({ status: "cancelled" });
        expect(invoke).toHaveBeenCalledWith("mobile_file_action", { action: "save", payload: { name: "a.png", mime: "image/png", data: "AH+A/w==" } });
    });

    it("should reject exports above 32 MiB before invoking native code", () => {
        expect(() => encodeMobileExport(new Uint8Array(32 * 1024 * 1024 + 1))).toThrow("32 MiB");
        expect(invoke).not.toHaveBeenCalled();
    });
});
