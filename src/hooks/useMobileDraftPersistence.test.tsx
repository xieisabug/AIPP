import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { invoke, clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useMobileDraftPersistence } from "./useMobileDraftPersistence";

vi.mock("./use-platform", () => ({ useIsMobilePlatform: () => true }));
vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

describe("mobile local draft persistence", () => {
    beforeEach(() => { clearAllMockHandlers(); vi.mocked(invoke).mockClear(); });
    it("should not replace new typing when an older draft read resolves", async () => {
        let resolve!: (value: string) => void;
        mockInvokeHandler("get_mobile_draft", () => new Promise<string>(done => { resolve = done; }));
        mockInvokeHandler("save_mobile_draft", () => undefined);
        const restore = vi.fn();
        const { result } = renderHook(() => useMobileDraftPersistence("late-read", restore));
        await waitFor(() => expect(resolve).toBeDefined());
        await act(async () => { result.current("new input"); resolve("old input"); });
        expect(restore).not.toHaveBeenCalled();
        expect(invoke).toHaveBeenCalledWith("save_mobile_draft", { conversationKey: "late-read", content: "new input" });
    });
    it("should write clear after content when earlier saves are still pending", async () => {
        mockInvokeHandler("get_mobile_draft", () => "");
        const saved: string[] = [];
        let first!: () => void;
        mockInvokeHandler("save_mobile_draft", (args: any) => {
            saved.push(args.content);
            if (args.content) return new Promise<void>(done => { first = done; });
        });
        const { result } = renderHook(() => useMobileDraftPersistence("ordered", vi.fn()));
        await act(async () => { result.current("draft"); result.current(""); });
        expect(saved).toEqual(["draft"]);
        await act(async () => { first(); });
        await waitFor(() => expect(saved).toEqual(["draft", ""]));
    });
});
