import { beforeEach, describe, expect, it, vi } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invokeMock(...args) }));

import { searchButlerMessages } from "./butlerSearchService";

const row = (overrides: Record<string, unknown>) => ({
    message_id: 1, conversation_id: 10, conversation_kind: "butler_main", conversation_name: "总管家主会话",
    bot_id: 1, bot_name: "总管家", role: "user", speaker: "用户", created_time: "2026-11-07 10:00:00", snippet: "周报",
    ...overrides,
});

describe("searchButlerMessages", () => {
    beforeEach(() => invokeMock.mockReset());

    it("should skip the backend when query is blank", async () => {
        expect(await searchButlerMessages("   ")).toEqual([]);
        expect(invokeMock).not.toHaveBeenCalled();
    });

    it("should label groups by butler conversation kind when mapping hits", async () => {
        invokeMock.mockResolvedValue([
            row({}),
            row({ message_id: 2, conversation_id: 11, conversation_name: "总管家主会话（归档）" }),
            row({ message_id: 3, conversation_id: 20, conversation_kind: "butler_contact", conversation_name: "写作", bot_id: 2, bot_name: "写作", speaker: "审稿" }),
            row({ message_id: 4, conversation_id: 30, conversation_kind: "butler_task", conversation_name: "整理周报" }),
            row({ message_id: 5, conversation_id: 21, conversation_kind: "butler_contact_archive", conversation_name: "与写作的私聊（存档 2026-11-01 08:00:00）", bot_id: 2, bot_name: "写作" }),
        ]);
        const hits = await searchButlerMessages(" 周报 ", 50, 0, 10);
        expect(invokeMock).toHaveBeenCalledWith("search_butler_messages", { query: "周报", limit: 50, offset: 0 });
        expect(hits.map(h => h.conversation_name)).toEqual(["当前主会话", "总管家主会话（归档）", "私聊", "派工任务「整理周报」", "与写作的私聊（存档 2026-11-01 08:00:00）"]);
        expect(hits[2]).toMatchObject({ assistant_id: 2, assistant_name: "写作", speaker: "审稿", hit_type: "message" });
        expect(hits[4]).toMatchObject({ conversation_kind: "butler_contact_archive", assistant_id: 2 });
    });
});
