import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import AssistantItemContextMenu from "./AssistantItemContextMenu";

const assistant = { id: 8, name: "目标助手", assistant_type: 0 };
describe("AssistantItemContextMenu", () => {
    it("should target the right-clicked assistant and omit history clearing in settings", () => {
        const action = vi.fn();
        render(<AssistantItemContextMenu assistant={assistant} onAction={action}><button>目标助手</button></AssistantItemContextMenu>);
        fireEvent.contextMenu(screen.getByRole("button", { name: "目标助手" }));
        expect(screen.queryByRole("menuitem", { name: "清空消息历史" })).toBeNull();
        fireEvent.click(screen.getByRole("menuitem", { name: "编辑" }));
        expect(action).toHaveBeenCalledWith(assistant, "edit");
    });
    it("should include history clearing only in Butler and preserve protected assistant restrictions", () => {
        const action = vi.fn();
        render(<AssistantItemContextMenu assistant={{ ...assistant, id: 1 }} onAction={action} canClear><button>快速助手</button></AssistantItemContextMenu>);
        fireEvent.contextMenu(screen.getByRole("button", { name: "快速助手" }));
        expect(screen.getByRole("menuitem", { name: "复制" })).toHaveAttribute("data-disabled");
        expect(screen.getByRole("menuitem", { name: "删除" })).toHaveAttribute("data-disabled");
        fireEvent.click(screen.getByRole("menuitem", { name: "清空消息历史" }));
        expect(action).toHaveBeenCalledWith({ ...assistant, id: 1 }, "clear");
    });
});
