import { beforeEach, describe, expect, it, vi } from "vitest";
import { toast } from "sonner";
import { conversationExportService, assertExportCanvasSize } from "./conversationExportService";
import { isAndroid, mobileFileAction, outputMobileFile } from "./mobileFileService";

vi.mock("./mobileFileService", () => ({ fileErrorMessage: (error: unknown) => error instanceof Error ? error.message : String(error), isAndroid: vi.fn(), mobileFileAction: vi.fn(), outputMobileFile: vi.fn() }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn(), info: vi.fn() } }));

describe("Android conversation export", () => {
    beforeEach(() => { vi.clearAllMocks(); vi.mocked(isAndroid).mockResolvedValue(true); });

    it("should return cancellation without reporting success when the save picker is cancelled", async () => {
        vi.mocked(outputMobileFile).mockResolvedValue({ status: "cancelled" });
        await expect(conversationExportService.saveFile(new Uint8Array([1]), "test.md", [])).resolves.toBeNull();
        expect(toast.success).not.toHaveBeenCalled();
    });

    it("should propagate native save errors without reporting success", async () => {
        vi.mocked(outputMobileFile).mockRejectedValue(new Error("save-write: provider failed"));
        await expect(conversationExportService.saveFile(new Uint8Array([1]), "test.md", [])).rejects.toThrow("provider failed");
        expect(toast.success).not.toHaveBeenCalled();
    });

    it("should reject oversize PNG dimensions before rendering", () => {
        expect(() => assertExportCanvasSize(800, 5000)).toThrow("PNG 安全尺寸");
        expect(toast.success).not.toHaveBeenCalled();
    });

    it("should not report a generated PDF when the system print dialog opens", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "print_launched" });
        await expect(conversationExportService.printMobilePdf("hello", "test.pdf")).resolves.toBe(false);
        expect(toast.success).not.toHaveBeenCalled();
        expect(toast.info).toHaveBeenCalled();
    });

    it("should reject an unsuccessful print launch without reporting success", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "cancelled" });
        await expect(conversationExportService.printMobilePdf("hello", "test.pdf")).rejects.toThrow("打印未调起");
        expect(toast.success).not.toHaveBeenCalled();
    });

    it("should show the native print failure when exporting a message PDF", async () => {
        vi.mocked(mobileFileAction).mockRejectedValue(new Error("MOBILE_FILE print-launch: PrintManager unavailable"));
        await expect(conversationExportService.exportSingleMessageToPDF("hello", "assistant")).rejects.toThrow("PrintManager unavailable");
        expect(toast.error).toHaveBeenCalledWith("PDF 导出失败: MOBILE_FILE print-launch: PrintManager unavailable");
        expect(toast.success).not.toHaveBeenCalled();
    });
});
