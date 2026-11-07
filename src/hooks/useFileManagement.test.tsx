import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { isAndroid, mobileFileAction } from "@/services/mobileFileService";
import { toast } from "sonner";
import useFileManagement from "./useFileManagement";
import { AttachmentType } from "../data/Conversation";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/services/mobileFileService", () => ({ fileErrorMessage: (error: unknown) => error instanceof Error ? error.message : String(error), isAndroid: vi.fn(), mobileFileAction: vi.fn() }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

describe("mobile attachment selection", () => {
    beforeEach(() => { vi.clearAllMocks(); vi.mocked(isAndroid).mockResolvedValue(true); });

    it("should leave attachments unchanged when the picker is cancelled", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "cancelled" });
        const selected = vi.fn();
        const { result } = renderHook(() => useFileManagement(selected));
        await act(async () => { await result.current.handleChooseFile(); });
        expect(result.current.fileInfoList).toBeNull();
        expect(invoke).not.toHaveBeenCalled();
        expect(selected).not.toHaveBeenCalled();
    });

    it("should retain successful registrations and report import and registration failures", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "selected", errors: ["batch exceeds 50 MiB"], files: [
            { path: "/ok", name: "ok.webp", mime: "image/webp", size: 1, thumbnail: "data:image/png;base64,AA==" },
            { path: "/bad", name: "bad.txt", mime: "text/plain", size: 1 },
            { path: "/invalid", name: "invalid.txt", mime: "text/plain", size: 1 },
        ] });
        vi.mocked(invoke).mockResolvedValueOnce({ attachment_id: 7 }).mockRejectedValueOnce(new Error("database unavailable")).mockResolvedValueOnce({ attachment_id: -1 });
        const selected = vi.fn();
        const { result } = renderHook(() => useFileManagement(selected));
        await act(async () => { await result.current.handleChooseFile(); });
        expect(result.current.fileInfoList).toEqual([{ id: 7, name: "ok.webp", path: "/ok", type: AttachmentType.Image, thumbnail: "data:image/png;base64,AA==" }]);
        expect(selected).toHaveBeenCalledWith(result.current.fileInfoList);
        expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("batch exceeds 50 MiB"));
        expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("database unavailable"));
        expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("有效 ID"));
    });

    it("should show a picker failure without creating an attachment", async () => {
        vi.mocked(mobileFileAction).mockRejectedValue(new Error("permission denied"));
        const { result } = renderHook(() => useFileManagement());
        await act(async () => { await result.current.handleChooseFile(); });
        expect(result.current.fileInfoList).toBeNull();
        expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("permission denied"));
    });
    it("should preserve a newly selected attachment when a draft finishes loading", async () => {
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "selected", files: [
            { path: "/new", name: "new.png", mime: "image/png", size: 1 },
        ] });
        vi.mocked(invoke).mockResolvedValue({ attachment_id: 99 });
        const { result } = renderHook(() => useFileManagement());
        act(() => result.current.restoreFiles([]));
        await act(async () => { await result.current.handleChooseFile(); });
        act(() => result.current.restoreDraftFiles([
            { id: 7, name: "old.png", path: "/old", type: AttachmentType.Image },
        ]));
        expect(result.current.fileInfoList?.map(file => file.id)).toEqual([99]);
    });

    it("should not resurrect attachments cleared during draft loading and should reset on conversation change", () => {
        const stored = [{ id: 7, name: "old.png", path: "/old", type: AttachmentType.Image }];
        const { result } = renderHook(() => useFileManagement());
        act(() => result.current.clearFileInfoList());
        act(() => result.current.restoreDraftFiles(stored));
        expect(result.current.fileInfoList).toBeNull();
        act(() => result.current.restoreFiles([]));
        act(() => result.current.restoreDraftFiles(stored));
        expect(result.current.fileInfoList).toEqual(stored);
    });

});
