import { fireEvent, render, screen, waitFor, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ButlerProfileDialog from "./ButlerProfileDialog";

const memoryApi = vi.hoisted(() => ({
    listButlerMemories: vi.fn(), updateButlerMemory: vi.fn(), deleteButlerMemory: vi.fn(),
    getButlerMemorySource: vi.fn(), onButlerMemoryChanged: vi.fn(),
}));
vi.mock("@/services/butlerMemoryService", () => memoryApi);

describe("Butler profile editor", () => {
    afterEach(cleanup);

    it("should list the assistant's memories in a separate tab when a conversation is given", async () => {
        memoryApi.listButlerMemories.mockResolvedValue([{ id: 5, content: "用户希望回答先给结论", scope_type: "assistant", scope_id: 2,
            author_assistant_id: 2, source_message_id: 9, source_group_id: null, source_group_seq: null, revision: 1, created_at: 0, updated_at: 0 }]);
        memoryApi.onButlerMemoryChanged.mockResolvedValue(() => {});
        render(<ButlerProfileDialog name="小A" memoryConversationId={20} onSave={vi.fn()} onClose={vi.fn()} />);
        expect(memoryApi.listButlerMemories).not.toHaveBeenCalled();
        fireEvent.mouseDown(screen.getByRole("tab", { name: "长期记忆" }), { button: 0 });
        expect(await screen.findByText("用户希望回答先给结论")).toBeInTheDocument();
        expect(memoryApi.listButlerMemories).toHaveBeenCalledWith(20);
    });

    it("should save trimmed name and independent colors when confirming the preview", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={onSave} onClose={onClose} />);
        fireEvent.change(screen.getByLabelText("助手名称"), { target: { value: " 小管家 " } });
        fireEvent.click(screen.getByRole("button", { name: "左侧 · 亮色：薄荷绿" }));
        fireEvent.click(screen.getByRole("button", { name: "右侧 · 深色：海军蓝" }));
        expect(onSave).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
        expect(onSave).toHaveBeenCalledWith("小管家", {
            version: 2, shape: "aipp", colors: { primary: "#CEF3DF", secondary: "#193B68" },
            accessories: { eyes: null, head: null }, motion: true,
        });
    });

    it("should discard edits when cancelled and reject blank names", () => {
        const onSave = vi.fn();
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={onSave} onClose={onClose} />);
        fireEvent.change(screen.getByLabelText("助手名称"), { target: { value: "  " } });
        expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
        fireEvent.click(screen.getByRole("button", { name: "取消" }));
        expect(onSave).not.toHaveBeenCalled();
        expect(onClose).toHaveBeenCalledOnce();
    });

    it("should preserve draft and show the underlying error when saving fails", async () => {
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={vi.fn().mockRejectedValue(new Error("配置写入失败：database is locked"))} onClose={onClose} />);
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
        expect(screen.getByLabelText("助手名称")).toHaveValue("总管家");
        expect(onClose).not.toHaveBeenCalled();
    });

    it("should retain accessories when changing shape and restoring colors", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="助手" onSave={onSave} onClose={vi.fn()} />);
        fireEvent.click(screen.getByRole("button", { name: "脸部：圆框眼镜" }));
        fireEvent.click(screen.getByRole("button", { name: "头部：贝雷帽" }));
        fireEvent.click(screen.getByRole("button", { name: "圆团" }));
        fireEvent.click(screen.getByRole("button", { name: "脸部 · 亮色：蜜桃" }));
        fireEvent.click(screen.getByRole("button", { name: "恢复黑白配色" }));
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("助手", {
            version: 2, shape: "pebble", colors: { primary: "#FFFFFF", secondary: "#000000" },
            accessories: { eyes: "round-glasses", head: "beret" }, motion: true,
        }));
    });

    it("should offer face and head decorations without manual preview controls", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="助手" appearance={{ version: 2, shape: "aipp", colors: { primary: "#FAD4E4", secondary: "#000000" },
            accessories: { eyes: null, head: null }, motion: false }} onSave={onSave} onClose={vi.fn()} />);
        expect(screen.queryByRole("checkbox")).toBeNull();
        expect(screen.queryByRole("combobox")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "脸部：墨镜" }));
        fireEvent.click(screen.getByRole("button", { name: "头部：铃铛蝴蝶结" }));
        expect(screen.getByRole("button", { name: "头部：铃铛蝴蝶结" })).toHaveAttribute("aria-pressed", "true");
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("助手", {
            version: 2, shape: "aipp", colors: { primary: "#FAD4E4", secondary: "#000000" },
            accessories: { eyes: "sunglasses", head: "bow-bell" }, motion: true,
        }));
    });
});
