import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ButlerMemory } from "@/data/ButlerMemory";
import ButlerMemoryPanel from "./ButlerMemoryPanel";

const api = vi.hoisted(() => ({
    listButlerMemories: vi.fn(), updateButlerMemory: vi.fn(), deleteButlerMemory: vi.fn(),
    getButlerMemorySource: vi.fn(), onButlerMemoryChanged: vi.fn(),
}));
vi.mock("@/services/butlerMemoryService", () => api);

function memory(patch: Partial<ButlerMemory>): ButlerMemory {
    return {
        id: 1, content: "用户希望回答先给结论", scope_type: "assistant", scope_id: 2, author_assistant_id: 2,
        source_message_id: 11, source_group_id: null, source_group_seq: null, revision: 1,
        created_at: Date.UTC(2026, 10, 9), updated_at: Date.UTC(2026, 10, 9), ...patch,
    };
}

const own = memory({});
const shared = memory({ id: 2, content: "所有回答使用中文", scope_type: "global", scope_id: null, revision: 3 });

beforeEach(() => {
    api.listButlerMemories.mockResolvedValue([own, shared]);
    api.onButlerMemoryChanged.mockResolvedValue(() => {});
});

afterEach(() => {
    cleanup();
    vi.clearAllMocks();
});

const item = (text: string) => screen.getByText(text).closest("li") as HTMLElement;

describe("ButlerMemoryPanel", () => {
    it("should group own and global memories when loaded for a conversation", async () => {
        render(<ButlerMemoryPanel conversationId={20} ownScopeLabel="该助手" />);
        expect(await screen.findByText("用户希望回答先给结论")).toBeInTheDocument();
        expect(api.listButlerMemories).toHaveBeenCalledWith(20);
        expect(screen.getByText("仅该助手 · 1")).toBeInTheDocument();
        expect(screen.getByText("全局 · 所有助手可见 · 1")).toBeInTheDocument();
    });

    it("should save an edit with the revision it was read at", async () => {
        api.updateButlerMemory.mockResolvedValue({ ...shared, content: "回答使用简体中文", revision: 4 });
        render(<ButlerMemoryPanel conversationId={20} ownScopeLabel="该助手" />);
        await screen.findByText("所有回答使用中文");
        fireEvent.click(within(item("所有回答使用中文")).getByRole("button", { name: "修改记忆 #2" }));
        fireEvent.change(screen.getByLabelText("编辑记忆 #2"), { target: { value: " 回答使用简体中文 " } });
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(api.updateButlerMemory).toHaveBeenCalledWith(2, "回答使用简体中文", 3));
        await waitFor(() => expect(screen.queryByLabelText("编辑记忆 #2")).not.toBeInTheDocument());
        expect(api.listButlerMemories).toHaveBeenCalledTimes(2);
    });

    it("should delete only after confirmation and show the underlying error when it fails", async () => {
        api.deleteButlerMemory.mockRejectedValue("记忆 #1 已被修改（当前版本 2），请重新读取后再操作");
        render(<ButlerMemoryPanel conversationId={20} ownScopeLabel="该助手" />);
        await screen.findByText("用户希望回答先给结论");
        fireEvent.click(within(item("用户希望回答先给结论")).getByRole("button", { name: "删除记忆 #1" }));
        expect(api.deleteButlerMemory).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "确认删除" }));
        await waitFor(() => expect(api.deleteButlerMemory).toHaveBeenCalledWith(1, 1));
        expect(await screen.findByRole("alert")).toHaveTextContent("删除记忆 #1 失败：记忆 #1 已被修改");
    });

    it("should show where a memory came from and when the source was deleted", async () => {
        api.getButlerMemorySource.mockResolvedValueOnce({ status: "available", label: "与小A的私聊", text: "记住，回答先给结论", truncated: false })
            .mockResolvedValueOnce({ status: "deleted", label: null, text: null, truncated: false });
        render(<ButlerMemoryPanel conversationId={20} ownScopeLabel="该助手" />);
        await screen.findByText("用户希望回答先给结论");
        fireEvent.click(within(item("用户希望回答先给结论")).getByRole("button", { name: "来源" }));
        expect(await screen.findByText("记住，回答先给结论")).toBeInTheDocument();
        expect(screen.getByText("与小A的私聊")).toBeInTheDocument();
        fireEvent.click(within(item("所有回答使用中文")).getByRole("button", { name: "来源" }));
        expect(await screen.findByText("原消息已删除，记忆仍然保留")).toBeInTheDocument();
    });

    it("should explain a load failure with the conversation id", async () => {
        api.listButlerMemories.mockRejectedValue("会话 #40 不是管家会话，没有长期记忆");
        render(<ButlerMemoryPanel conversationId={40} ownScopeLabel="该助手" />);
        expect(await screen.findByRole("alert")).toHaveTextContent("读取长期记忆失败（会话 40）：会话 #40 不是管家会话");
    });
});
