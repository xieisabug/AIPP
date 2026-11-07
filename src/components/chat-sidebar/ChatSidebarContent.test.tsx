import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ChatSidebarContent from "./ChatSidebarContent";
import type { LoadedPlugin } from "@/services/PluginRuntime";

vi.mock("@tauri-apps/plugin-clipboard-manager", () => ({ writeText: vi.fn() }));

const originalInnerWidth = window.innerWidth;
afterEach(() => {
    Object.defineProperty(window, "innerWidth", { configurable: true, value: originalInnerWidth });
});

describe("ChatSidebarContent mobile details", () => {
    it("should render all sections with readable content and 44px mobile controls at 360px", () => {
        Object.defineProperty(window, "innerWidth", { configurable: true, value: 360 });
        const plugin = {
            pluginId: 1,
            code: "fixture",
            name: "Fixture",
            version: "1.0.0",
            pluginType: ["applicationType"],
            instance: null,
            loadError: "plugin_id=1; stage=load: fixture failed",
            contributions: { views: [{ id: "details", location: "conversation.sidebar", title: "插件详情" }] },
        } as LoadedPlugin;
        render(
            <ChatSidebarContent
                mobile
                conversationId="7"
                todos={[{ content: "完成移动详情验收", status: "in_progress", activeForm: "正在验收" }]}
                artifacts={[{ id: "artifact-1", language: "text", code: "content", title: "Example" }]}
                contextItems={[{
                    id: "search-1",
                    type: "search",
                    name: "移动详情长搜索结果",
                    source: "mcp",
                    searchResults: [{ title: "第一条结果", url: "https://example.com", snippet: "结果内容" }],
                }]}
                pluginList={[plugin]}
            />,
        );

        expect(screen.getByText("完成移动详情验收")).toBeInTheDocument();
        expect(screen.getByText("Example")).toBeInTheDocument();
        expect(screen.getByText("移动详情长搜索结果")).toBeInTheDocument();
        expect(screen.getByRole("alert").textContent).toContain("fixture failed");
        for (const title of ["规划", "Artifact", "上下文", "插件视图"]) {
            expect(screen.getByRole("button", { name: new RegExp(title) })).toHaveClass("min-h-11");
        }
        const copy = screen.getByRole("button", { name: "复制 Example" });
        expect(copy).toHaveClass("h-11", "w-11");
        expect(copy.parentElement).not.toHaveClass("opacity-0");
        expect(screen.getByRole("button", { name: "展开搜索结果" })).toHaveClass("h-11", "w-11");
    });

    it("should explain empty mobile plugins without changing desktop section visibility", () => {
        const props = { todos: [], artifacts: [], contextItems: [] };
        const { rerender } = render(<ChatSidebarContent {...props} mobile />);
        expect(screen.getByText(/插件仅可使用已通过移动校验的能力/)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "插件视图" })).toHaveClass("min-h-11");

        rerender(<ChatSidebarContent {...props} />);
        expect(screen.queryByRole("button", { name: "插件视图" })).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "规划" })).not.toHaveClass("min-h-11");
    });
});
