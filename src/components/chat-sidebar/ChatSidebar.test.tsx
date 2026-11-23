import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ChatSidebar from "./ChatSidebar";

const platformState = vi.hoisted(() => ({ platform: "windows" as string }));
vi.mock("@/hooks/use-platform", async (importOriginal) => ({
    ...(await importOriginal<typeof import("@/hooks/use-platform")>()),
    usePlatform: () => platformState.platform,
}));

const todos = [{ content: "整理纪要", status: "pending" as const, activeForm: "整理纪要" }];

describe("ChatSidebar window controls", () => {
    beforeEach(() => {
        platformState.platform = "windows";
    });

    it("should carry the Windows window controls in its header only while expanded", () => {
        const onExpandChange = vi.fn();
        render(<ChatSidebar todos={todos} artifacts={[]} contextItems={[]} conversationId="1"
            showWindowControls onExpandChange={onExpandChange} />);

        const header = document.querySelector('[data-aipp-slot="chat-inline-sidebar-header"]') as HTMLElement;
        expect(header).toHaveAttribute("data-tauri-drag-region");
        expect(header).toContainElement(screen.getByRole("button", { name: "关闭" }));

        fireEvent.click(screen.getByTitle("收起"));
        // 收起后由会话标题栏显示按钮（见 ConversationUI.showWindowControls）
        expect(screen.queryByRole("button", { name: "关闭" })).toBeNull();
        expect(onExpandChange).toHaveBeenLastCalledWith(false, expect.any(Number));
    });

    it("should not draw window controls when it is not at the window corner", () => {
        render(<ChatSidebar todos={todos} artifacts={[]} contextItems={[]} conversationId="1" />);
        expect(screen.queryByRole("button", { name: "关闭" })).toBeNull();
        expect(document.querySelector('[data-aipp-slot="chat-inline-sidebar-header"]')).not.toHaveAttribute("data-tauri-drag-region");
    });
});
