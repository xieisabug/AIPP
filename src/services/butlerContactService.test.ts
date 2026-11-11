import { afterEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import { resetButlerContactConversation } from "./butlerContactService";

describe("resetButlerContactConversation", () => {
    afterEach(clearAllMockHandlers);
    it("should archive through the backend without deleting the conversation", async () => {
        const reset = vi.fn(() => ({ conversationId: 21, archivedConversationIds: [20], stopNotes: [] }));
        const remove = vi.fn();
        mockInvokeHandler("reset_butler_contact_conversation", reset);
        mockInvokeHandler("delete_conversation", remove);
        expect(await resetButlerContactConversation(2)).toEqual({ conversationId: 21, archivedConversationIds: [20], stopNotes: [] });
        expect(reset).toHaveBeenCalledExactlyOnceWith({ assistantId: 2 });
        expect(remove).not.toHaveBeenCalled();
    });
    it("should pass through the backend stage and cause when archiving fails", async () => {
        mockInvokeHandler("reset_butler_contact_conversation", () => { throw new Error("归档助手 2 的旧私聊出错：database is locked"); });
        await expect(resetButlerContactConversation(2)).rejects.toThrow("归档助手 2 的旧私聊出错：database is locked");
    });
});
