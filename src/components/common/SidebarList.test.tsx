import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import SidebarList from "./SidebarList";
import ListItemButton from "./ListItemButton";

describe("SidebarList", () => {
    it("should keep search and page actions in a toolbar without repeating the page title", () => {
        const onSearchChange = vi.fn();
        render(
            <SidebarList title="提供商" description="选择提供商进行配置" icon={<span>icon</span>} searchValue="" onSearchChange={onSearchChange}
                searchPlaceholder="搜索提供商..." addButton={<button type="button">导入</button>}>
                <ListItemButton isSelected onClick={() => {}}>OpenAI</ListItemButton>
            </SidebarList>,
        );
        const list = screen.getByRole("navigation", { name: "提供商" });
        expect(list).toHaveAttribute("data-aipp-slot", "config-list");
        expect(screen.queryByText("选择提供商进行配置")).not.toBeInTheDocument();
        expect(screen.queryByText("icon")).not.toBeInTheDocument();
        fireEvent.change(screen.getByPlaceholderText("搜索提供商..."), { target: { value: "open" } });
        expect(onSearchChange).toHaveBeenCalledWith("open");
        expect(screen.getByRole("button", { name: "导入" }).closest('[data-aipp-slot="config-list-actions"]')).not.toBeNull();
    });

    it("should omit the toolbar when a page has neither search nor actions", () => {
        const { container } = render(
            <SidebarList title="程序功能"><ListItemButton isSelected={false} onClick={() => {}}>显示</ListItemButton></SidebarList>,
        );
        expect(container.querySelector('[data-aipp-slot="config-list-toolbar"]')).toBeNull();
    });
});

describe("ListItemButton", () => {
    it("should mark the selected row and report clicks", () => {
        const onClick = vi.fn();
        render(
            <>
                <ListItemButton isSelected onClick={() => {}}>A</ListItemButton>
                <ListItemButton isSelected={false} onClick={onClick}>B</ListItemButton>
            </>,
        );
        expect(screen.getByRole("button", { name: "A" })).toHaveAttribute("aria-current", "true");
        expect(screen.getByRole("button", { name: "B" })).not.toHaveAttribute("aria-current");
        // 字重由组件统一，选中与否、哪个页面都一样，避免各页面列表粗细不一致。
        expect(screen.getByRole("button", { name: "A" })).toHaveClass("font-medium");
        expect(screen.getByRole("button", { name: "B" })).toHaveClass("font-medium");
        fireEvent.click(screen.getByRole("button", { name: "B" }));
        expect(onClick).toHaveBeenCalledOnce();
    });
});
