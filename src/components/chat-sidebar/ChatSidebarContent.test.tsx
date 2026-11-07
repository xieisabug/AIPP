import React from "react";
import { useMobileConversationDetails } from "@/hooks/useMobileConversationDetails";
import { runMobileBackHandlers } from "@/mobile/navigation";
import { fireEvent, render, screen } from "@testing-library/react";
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

const longArtifacts = Array.from({ length: 17 }, (_, i) => ({
    id: `artifact-${i}`, language: "html", code: `<h1>${i}</h1>`, title: `Artifact item ${i}`,
}));
const longContext = Array.from({ length: 20 }, (_, i) => ({
    id: `context-${i}`, type: "preview_file" as const, source: "mcp" as const, name: `Context item ${i}`,
}));
const todos = [{ content: "Unfinished todo", status: "pending" as const, activeForm: "Planning" }];

function DetailsHarness({ conversationId = "7" }: { conversationId?: string }) {
    const [previewFileOpen, setPreviewFileOpen] = React.useState(false);
    const details = useMobileConversationDetails({ conversationId, enabled: true, previewFileOpen });
    return <>
        <button onClick={details.openDetails}>Open details</button>
        <button onClick={() => runMobileBackHandlers("preview")}>Preview back</button>
        <button onClick={() => setPreviewFileOpen(false)}>Close context preview</button>
        {details.detailsOpen && <div data-testid="details-root" ref={details.detailsScrollRef}>
            <button onClick={() => details.setDetailsOpen(false)}>Close details</button>
            <ChatSidebarContent mobile conversationId={conversationId}
                todos={todos} artifacts={longArtifacts} contextItems={longContext}
                restoreState={details.detailsRestoreState}
                onArtifactClick={() => details.suspendDetails("artifact")}
                onPreviewFileClick={() => {
                    details.suspendDetails("context");
                    setPreviewFileOpen(true);
                }} />
        </div>}
    </>;
}

function sectionScroller(id: string) {
    return document.querySelector<HTMLDivElement>(`[data-chat-sidebar-scroll="${id}"]`)!;
}
function sectionToggle(id: string) {
    return document.querySelector<HTMLButtonElement>(`[data-chat-sidebar-section="${id}"] > button`)!;
}

describe("ChatSidebarContent preview restoration integration", () => {
    it("should restore independent nested offsets and folded nonempty sections after artifact preview", () => {
        render(<React.StrictMode><DetailsHarness /></React.StrictMode>);
        fireEvent.click(screen.getByText("Open details"));
        fireEvent.click(sectionToggle("todo"));
        fireEvent.click(sectionToggle("plugin"));
        sectionScroller("artifact").scrollTop = 500;
        sectionScroller("context").scrollTop = 210;
        screen.getByTestId("details-root").scrollTop = 15;
        fireEvent.click(screen.getByText("Artifact item 7"));
        expect(screen.queryByTestId("details-root")).not.toBeInTheDocument();
        fireEvent.click(screen.getByText("Preview back"));
        expect(sectionScroller("artifact").scrollTop).toBe(500);
        expect(sectionScroller("context").scrollTop).toBe(210);
        expect(screen.getByTestId("details-root").scrollTop).toBe(15);
        expect(sectionToggle("todo")).toHaveAttribute("aria-expanded", "false");
        expect(sectionToggle("plugin")).toHaveAttribute("aria-expanded", "false");
        expect(sectionToggle("artifact")).toHaveAttribute("aria-expanded", "true");

        // A later preview must capture the new reading position, not replay the old snapshot.
        sectionScroller("artifact").scrollTop = 720;
        fireEvent.click(screen.getByText("Artifact item 12"));
        fireEvent.click(screen.getByText("Preview back"));
        expect(sectionScroller("artifact").scrollTop).toBe(720);
    });

    it("should restore the context section offset after its real content remounts", () => {
        render(<DetailsHarness />);
        fireEvent.click(screen.getByText("Open details"));
        sectionScroller("context").scrollTop = 430;
        fireEvent.click(sectionToggle("artifact"));
        fireEvent.click(screen.getByText("Context item 9"));
        expect(screen.queryByTestId("details-root")).not.toBeInTheDocument();
        fireEvent.click(screen.getByText("Close context preview"));
        expect(sectionScroller("context").scrollTop).toBe(430);
        expect(sectionToggle("artifact")).toHaveAttribute("aria-expanded", "false");
    });

    it("should discard a preview snapshot on conversation change or an ordinary reopen", () => {
        const { rerender } = render(<DetailsHarness conversationId="7" />);
        fireEvent.click(screen.getByText("Open details"));
        sectionScroller("artifact").scrollTop = 500;
        fireEvent.click(screen.getByText("Artifact item 7"));
        rerender(<DetailsHarness conversationId="8" />);
        fireEvent.click(screen.getByText("Preview back"));
        expect(screen.queryByTestId("details-root")).not.toBeInTheDocument();
        fireEvent.click(screen.getByText("Open details"));
        expect(sectionScroller("artifact").scrollTop).toBe(0);
        sectionScroller("artifact").scrollTop = 250;
        fireEvent.click(screen.getByText("Artifact item 4"));
        fireEvent.click(screen.getByText("Preview back"));
        fireEvent.click(screen.getByText("Close details"));
        fireEvent.click(screen.getByText("Open details"));
        expect(sectionScroller("artifact").scrollTop).toBe(0);
        expect(sectionToggle("todo")).toHaveAttribute("aria-expanded", "true");
    });

    it("should ignore mobile restoration snapshots for the desktop sidebar", () => {
        render(<ChatSidebarContent todos={todos} artifacts={longArtifacts} contextItems={longContext}
            restoreState={{ todo: { isOpen: false, scrollTop: 99 }, artifact: { isOpen: false, scrollTop: 500 } }} />);
        expect(screen.getByText("Unfinished todo")).toBeVisible();
        expect(screen.getByText("Artifact item 7")).toBeVisible();
        expect(document.querySelector("[data-chat-sidebar-section]")).toBeNull();
        expect(document.querySelector("[data-chat-sidebar-scroll]")).toBeNull();
    });
});
