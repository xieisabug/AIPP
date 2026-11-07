import { useEffect, useState } from "react";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { mobileFileAction } from "@/services/mobileFileService";
import { useConversationDraft } from "./useConversationDraft";
import useFileManagement from "./useFileManagement";

vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => true }));
vi.mock("@/services/mobileFileService", () => ({
    isAndroid: async () => true, mobileFileAction: vi.fn(), fileErrorMessage: String,
}));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

// Match ConversationUI's ordering: files are persisted only after draft hydration.
function useComposerDraft(key: string) {
    const files = useFileManagement();
    const [readyKey, setReadyKey] = useState<string | null>(null);
    const ready = readyKey === key;
    useEffect(() => {
        setReadyKey(null);
        files.restoreFiles([]);
    }, [key, files.restoreFiles]);
    const [text, setText, saveIds] = useConversationDraft(key, files.restoreDraftFiles, () => setReadyKey(key));
    useEffect(() => {
        if (ready) saveIds((files.fileInfoList ?? []).map(file => file.id));
    }, [ready, files.fileInfoList, saveIds]);
    return { ...files, text, setText, ready };
}

describe("mobile composer delayed draft", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        vi.mocked(mobileFileAction).mockReset();
        mockInvokeHandler("save_mobile_draft", () => undefined);
    });

    it("should keep and persist a new selection when stored attachments arrive after the picker", async () => {
        let resolve!: (draft: unknown) => void;
        mockInvokeHandler("get_mobile_draft", () => new Promise(done => { resolve = done; }));
        mockInvokeHandler("add_attachment", () => ({ attachment_id: 99 }));
        vi.mocked(mobileFileAction).mockResolvedValue({ status: "selected", files: [
            { name: "new.png", path: "/new", size: 1, mime: "image/png" },
        ] });
        const { result } = renderHook(() => useComposerDraft("composer-picker"));
        await waitFor(() => expect(resolve).toBeDefined());
        await act(async () => { await result.current.handleChooseFile(); });
        await act(async () => resolve({ content: "stored caption", attachments: [
            { id: 7, name: "old.png", path: "/old", attachment_type: 1 },
        ] }));
        expect(result.current.fileInfoList?.map(file => file.id)).toEqual([99]);
        expect(result.current.text).toBe("stored caption");
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_mobile_draft", {
            conversationKey: "composer-picker", content: "stored caption", attachmentIds: [99],
        }));
    });
    it("should never save the previous conversation's attachments into a newly selected key", async () => {
        let resolveNew!: (draft: unknown) => void;
        mockInvokeHandler("get_mobile_draft", (args) => args?.conversationKey === "switch-old"
            ? { content: "old text", attachments: [{ id: 7, name: "old.png", path: "/old", attachment_type: 1 }] }
            : new Promise(done => { resolveNew = done; }));
        const { result, rerender } = renderHook(({ id }) => useComposerDraft(id), {
            initialProps: { id: "switch-old" },
        });
        await waitFor(() => expect(result.current.ready).toBe(true));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_mobile_draft", {
            conversationKey: "switch-old", content: "old text", attachmentIds: [7],
        }));
        rerender({ id: "switch-new" });
        await waitFor(() => expect(resolveNew).toBeDefined());
        expect(result.current.ready).toBe(false);
        expect(result.current.fileInfoList).toBeNull();
        await act(async () => resolveNew({ content: "new stored text", attachments: [
            { id: 8, name: "new.png", path: "/new", attachment_type: 1 },
        ] }));
        expect(result.current.fileInfoList?.map(file => file.id)).toEqual([8]);
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("save_mobile_draft", {
            conversationKey: "switch-new", content: "new stored text", attachmentIds: [8],
        }));
        const newWrites = vi.mocked(invoke).mock.calls.filter(([command, args]) =>
            command === "save_mobile_draft" && args?.conversationKey === "switch-new");
        expect(newWrites.length).toBeGreaterThan(0);
        for (const [, args] of newWrites) expect(args?.attachmentIds).toEqual([8]);
    });

    it("should ignore a late read from the previous key after a conversation switch", async () => {
        const resolves = new Map<string, (draft: unknown) => void>();
        mockInvokeHandler("get_mobile_draft", (args) => new Promise(done => {
            resolves.set(String(args?.conversationKey), done);
        }));
        const { result, rerender } = renderHook(({ id }) => useComposerDraft(id), {
            initialProps: { id: "late-old" },
        });
        await waitFor(() => expect(resolves.has("late-old")).toBe(true));
        rerender({ id: "late-new" });
        await waitFor(() => expect(resolves.has("late-new")).toBe(true));
        await act(async () => resolves.get("late-new")?.({ content: "new text", attachments: [
            { id: 9, name: "new.png", path: "/new", attachment_type: 1 },
        ] }));
        await act(async () => resolves.get("late-old")?.({ content: "old text", attachments: [
            { id: 7, name: "old.png", path: "/old", attachment_type: 1 },
        ] }));
        expect(result.current.text).toBe("new text");
        expect(result.current.fileInfoList?.map(file => file.id)).toEqual([9]);
        for (const [command, args] of vi.mocked(invoke).mock.calls) {
            if (command === "save_mobile_draft" && args?.conversationKey === "late-new") {
                expect(args?.attachmentIds).toEqual([9]);
            }
        }
    });

});
