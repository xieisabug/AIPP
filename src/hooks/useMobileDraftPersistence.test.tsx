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
        });
        // Gated: no disk write until hydrate finishes
        expect(invoke).not.toHaveBeenCalledWith(
            "save_mobile_draft",
            expect.anything(),
        );
        await act(async () => {
            resolve({ content: "old input", attachments: [] });
        });
        expect(restore).not.toHaveBeenCalled();
        await waitFor(() =>
            expect(invoke).toHaveBeenCalledWith("save_mobile_draft", {
                conversationKey: "late-read",
                content: "new input",
                attachmentIds: [],
            }),
        );
    });

    it("should not persist empty attachmentIds before hydrate finishes", async () => {
        let resolve!: (value: {
            content: string;
            attachments: Array<{
                id: number;
                name: string;
                path: string;
                attachment_type: number;
            }>;
        }) => void;
        mockInvokeHandler(
            "get_mobile_draft",
            () => new Promise((done) => {
                resolve = done;
            }),
        );
        const saved: Array<{ content: string; attachmentIds: number[] }> = [];
        mockInvokeHandler("save_mobile_draft", (args: any) => {
            saved.push({ content: args.content, attachmentIds: args.attachmentIds ?? [] });
        });
        const restore = vi.fn();
        const { result } = renderHook(() => useMobileDraftPersistence("hydrate-gate", restore));
        await waitFor(() => expect(resolve).toBeDefined());
        await act(async () => {
            result.current.saveContent("typed during hydrate");
        });
        expect(saved).toEqual([]);
        await act(async () => {
            resolve({
                content: "old",
                attachments: [
                    { id: 7, name: "a.png", path: "a.png", attachment_type: 1 },
                    { id: 8, name: "b.txt", path: "b.txt", attachment_type: 0 },
                ],
            });
        });
        // Restore attachments into UI without clobbering typed content
        await waitFor(() => expect(restore).toHaveBeenCalled());
        expect(restore).toHaveBeenCalledWith({
            content: "typed during hydrate",
            attachments: [
                { id: 7, name: "a.png", path: "a.png", attachment_type: 1 },
                { id: 8, name: "b.txt", path: "b.txt", attachment_type: 0 },
            ],
        });
        await waitFor(() =>
            expect(saved).toEqual([
                { content: "typed during hydrate", attachmentIds: [7, 8] },
            ]),
        );
        // Never wrote empty attachmentIds
        expect(saved.some((row) => row.attachmentIds.length === 0)).toBe(false);
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
        const onReady = vi.fn();
        const { result } = renderHook(() =>
            useMobileDraftPersistence("ordered", vi.fn(), onReady),
        );
        await waitFor(() => expect(onReady).toHaveBeenCalled());
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
        const onReady = vi.fn();
        const { result } = renderHook(() =>
            useMobileDraftPersistence("with-files", vi.fn(), onReady),
        );
        await waitFor(() => expect(onReady).toHaveBeenCalled());
        await act(async () => {
            result.current.saveContent("caption");
            result.current.saveAttachmentIds([11, 12]);
        });
        await waitFor(() =>
            expect(saved).toContainEqual({ content: "caption", attachmentIds: [11, 12] }),
        );
    });
});
