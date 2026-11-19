import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import React from "react";
import SidebarListGroups from "./SidebarListGroups";

// Radix 下拉菜单走 portal，测试里换成直接渲染，只验证分组与跳转逻辑。
vi.mock("../ui/dropdown-menu", () => ({
    DropdownMenu: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    DropdownMenuTrigger: ({ children }: { children: React.ReactNode }) => <>{children}</>,
    DropdownMenuContent: ({ children }: { children: React.ReactNode }) => <div role="menu">{children}</div>,
    DropdownMenuItem: ({ children, onSelect }: { children: React.ReactNode; onSelect: () => void }) => (
        <button type="button" role="menuitem" onClick={onSelect}>{children}</button>
    ),
}));

const groups = [
    { key: "agents", label: "Agents", count: 2, children: [<div key="a">a</div>, <div key="b">b</div>] },
    { key: "codex", label: "Codex", count: 1, children: <div>c</div> },
];

describe("SidebarListGroups", () => {
    it("should render a sticky header with label and count for each group", () => {
        const { container } = render(<SidebarListGroups groups={groups} />);
        const headers = container.querySelectorAll('[data-aipp-slot="config-list-group-header"]');
        expect(headers).toHaveLength(2);
        expect(headers[0]).toHaveClass("sticky");
        expect(screen.getByRole("region", { name: "Agents" })).toHaveAttribute("data-aipp-slot", "config-list-group");
        expect(screen.getByRole("button", { name: "Agents，2 项，切换分组" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Codex，1 项，切换分组" })).toBeInTheDocument();
    });

    it("should scroll only the list to the chosen group when a group is picked from the header menu", () => {
        const { container } = render(
            <div data-aipp-slot="config-list-items"><SidebarListGroups groups={groups} /></div>,
        );
        const scroller = container.firstElementChild as HTMLElement;
        scroller.scrollTop = 40;
        const scrollTo = vi.fn();
        scroller.scrollTo = scrollTo as unknown as typeof scroller.scrollTo;
        scroller.getBoundingClientRect = () => ({ top: 100 } as DOMRect);
        screen.getByRole("region", { name: "Codex" }).getBoundingClientRect = () => ({ top: 300 } as DOMRect);

        // 每个分组标题的菜单都列出全部分组；在 Agents 的菜单里选 Codex。
        const agentsMenu = screen.getAllByRole("menu")[0];
        fireEvent.click(Array.from(agentsMenu.querySelectorAll('[role="menuitem"]')).find((item) => item.textContent?.includes("Codex"))!);

        expect(scrollTo).toHaveBeenCalledWith({ top: 240, behavior: "smooth" });
        expect(document.activeElement).toBe(screen.getByRole("button", { name: "Codex，1 项，切换分组" }));
    });

    it("should show a plain header without a switcher when there is only one group", () => {
        render(<SidebarListGroups groups={[groups[0]]} />);
        expect(screen.getByRole("region", { name: "Agents" })).toBeInTheDocument();
        expect(screen.queryByRole("button")).not.toBeInTheDocument();
        expect(screen.queryByRole("menu")).not.toBeInTheDocument();
    });
});
