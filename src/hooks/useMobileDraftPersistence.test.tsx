import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useMobileDraftPersistence } from "./useMobileDraftPersistence";

vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => true }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

describe("mobile local draft persistence", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
    });

    it("should not replace new typing when an older draft read resolves", async () => {
        let resolve!: (value: { content: string; attachments: unknown[] }) => void;
        mockInvokeHandler(
            "get_mobile_draft",
            () => new Promise((done) => {
                resolve = done;
            }),
        );
        mockInvokeHandler("save_mobile_draft", () => undefined);
        const restore = vi.fn();
        const { result } = renderHook(() => useMobileDraftPersistence("late-read", restore));
        await waitFor(() => expect(resolve).toBeDefined());
        await act(async () => {
            result.current.saveContent("new input");
            resolve({ content: "old input", attachments: [] });
        });
        expect(restore).not.toHaveBeenCalled();
        expect(invoke).toHaveBeenCalledWith("save_mobile_draft", {
            conversationKey: "late-read",
            content: "new input",
            attachmentIds: [],
        });
    });

    it("should write clear after content when earlier saves are still pending", async () => {
        mockInvokeHandler("get_mobile_draft", () => ({ content: "", attachments: [] }));
        const saved: Array<{ content: string; attachmentIds: number[] }> = [];
        let first!: () => void;
        mockInvokeHandler("save_mobile_draft", (args: any) => {
            saved.push({ content: args.content, attachmentIds: args.attachmentIds ?? [] });
            if (args.content) return new Promise<void>((done) => {
                first = done;
            });
        });
        const { result } = renderHook(() => useMobileDraftPersistence("ordered", vi.fn()));
        await act(async () => {
            result.current.saveContent("draft");
            result.current.saveContent("");
        });
        expect(saved).toEqual([{ content: "draft", attachmentIds: [] }]);
        await act(async () => {
            first();
        });
        await waitFor(() =>
            expect(saved).toEqual([
                { content: "draft", attachmentIds: [] },
                { content: "", attachmentIds: [] },
            ]),
        );
    });

    it("should persist attachment ids with the latest text", async () => {
        mockInvokeHandler("get_mobile_draft", () => ({ content: "", attachments: [] }));
        const saved: Array<{ content: string; attachmentIds: number[] }> = [];
        mockInvokeHandler("save_mobile_draft", (args: any) => {
            saved.push({ content: args.content, attachmentIds: args.attachmentIds ?? [] });
        });
        const { result } = renderHook(() => useMobileDraftPersistence("with-files", vi.fn()));
        await act(async () => {
            result.current.saveContent("caption");
            result.current.saveAttachmentIds([11, 12]);
        });
        await waitFor(() =>
            expect(saved).toContainEqual({ content: "caption", attachmentIds: [11, 12] }),
        );
    });
});
