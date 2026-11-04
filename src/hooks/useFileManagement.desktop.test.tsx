import { act, renderHook } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { readFile } from "@tauri-apps/plugin-fs";
import { toast } from "sonner";
import useFileManagement from "./useFileManagement";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-fs", () => ({ readFile: vi.fn() }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

describe("desktop attachment registration", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        vi.mocked(readFile).mockResolvedValue(new Uint8Array([1]));
    });

    it("should retain only successful attachments when one registration fails", async () => {
        vi.mocked(open).mockResolvedValue(["C:\\ok.txt", "C:\\bad.txt"]);
        vi.mocked(invoke).mockImplementation(async (_command, args) => {
            if (_command === "get_platform") return "windows" as never;
            if ((args as { fileUrl: string }).fileUrl.includes("bad")) throw new Error("database unavailable");
            return { attachment_id: 7 } as never;
        });
        const selected = vi.fn();
        const { result } = renderHook(() => useFileManagement(selected));
        await act(async () => { await result.current.handleChooseFile(); });
        expect(result.current.fileInfoList?.map(file => file.id)).toEqual([7]);
        expect(selected).toHaveBeenCalledWith(result.current.fileInfoList);
        expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("database unavailable"));
    });

    it("should add no invalid attachments when all registrations fail", async () => {
        vi.mocked(open).mockResolvedValue(["C:\\a.txt", "C:\\b.txt"]);
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === "get_platform") return "windows" as never;
            throw new Error("database unavailable");
        });
        const { result } = renderHook(() => useFileManagement());
        await act(async () => { await result.current.handleChooseFile(); });
        expect(result.current.fileInfoList).toEqual([]);
        expect(toast.error).toHaveBeenCalledTimes(2);
    });

    it("should reject nonpositive attachment IDs returned by the backend", async () => {
        vi.mocked(open).mockResolvedValue(["C:\\zero.txt", "C:\\negative.txt"]);
        let attachmentCalls = 0;
        vi.mocked(invoke).mockImplementation(async command => {
            if (command === "get_platform") return "windows" as never;
            return { attachment_id: attachmentCalls++ === 0 ? 0 : -1 } as never;
        });
        const { result } = renderHook(() => useFileManagement());
        await act(async () => { await result.current.handleChooseFile(); });
        expect(result.current.fileInfoList).toEqual([]);
        expect(toast.error).toHaveBeenCalledTimes(2);
        expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("有效 ID"));
    });
});
