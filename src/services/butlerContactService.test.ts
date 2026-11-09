import { afterEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import { clearButlerContactHistory } from "./butlerContactService";

describe("clearButlerContactHistory", () => {
    afterEach(clearAllMockHandlers);
    it("should stop and delete only the selected assistant private conversation", async () => {
        mockInvokeHandler("list_butler_contact_previews", () => [{ assistantId: 1, conversationId: 10 }, { assistantId: 2, conversationId: 20 }]);
        const order: string[] = [];
        const cancel = vi.fn(() => { order.push("cancel"); });
        const remove = vi.fn(() => { order.push("delete"); });
        mockInvokeHandler("cancel_ai", cancel);
        mockInvokeHandler("delete_conversation", remove);
        expect(await clearButlerContactHistory(2)).toBe(20);
        expect(order).toEqual(["cancel", "delete"]);
        expect(cancel).toHaveBeenCalledExactlyOnceWith({ conversationId: 20 });
        expect(remove).toHaveBeenCalledExactlyOnceWith({ conversationId: 20 });
    });
    it("should keep history when stopping fails and report the original cause", async () => {
        mockInvokeHandler("list_butler_contact_previews", () => [{ assistantId: 2, conversationId: 20 }]);
        mockInvokeHandler("cancel_ai", () => { throw new Error("session unavailable"); });
        const remove = vi.fn();
        mockInvokeHandler("delete_conversation", remove);
        await expect(clearButlerContactHistory(2)).rejects.toThrow("停止私聊执行失败（助手 2，会话 20）：Error: session unavailable");
        expect(remove).not.toHaveBeenCalled();
    });
    it("should do nothing when the assistant has no private conversation", async () => {
        mockInvokeHandler("list_butler_contact_previews", () => []);
        const remove = vi.fn();
        mockInvokeHandler("delete_conversation", remove);
        expect(await clearButlerContactHistory(2)).toBeNull();
        expect(remove).not.toHaveBeenCalled();
    });
});
