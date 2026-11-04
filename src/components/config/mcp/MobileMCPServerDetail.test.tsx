import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import MobileMCPServerDetail from "./MobileMCPServerDetail";
import type { MCPServer, MCPServerTool } from "@/data/MCP";

vi.mock("@/assets/mcp.svg?react", () => ({ default: () => <svg /> }));

const server = {
    id: 7, name: "search", description: "web search", transport_type: "stdio", command: "aipp:search",
    is_enabled: true, is_long_running: false, is_deletable: false, timeout: 30000,
} as unknown as MCPServer;
const tool = {
    id: 11, server_id: 7, tool_name: "web_search", tool_description: "Search the web",
    is_enabled: true, is_auto_run: false, parameters: JSON.stringify({ properties: { q: { type: "string" } } }),
} as unknown as MCPServerTool;

function renderDetail(overrides: Partial<React.ComponentProps<typeof MobileMCPServerDetail>> = {}) {
    const props = {
        server, tools: [tool], prompts: [], resources: [], isRefreshing: false,
        serverUnexecutable: false, isToolUnexecutable: () => false, expandedTools: new Set<number>(),
        onToggleToolExpansion: vi.fn(), onToggleServer: vi.fn(), onRefresh: vi.fn(), onEdit: vi.fn(),
        onDelete: vi.fn(), onUpdateTool: vi.fn(), onUpdatePrompt: vi.fn(), truncateText: (text: string) => text,
        ...overrides,
    };
    render(<MobileMCPServerDetail {...props} />);
    return props;
}

describe("MobileMCPServerDetail", () => {
    it("should toggle the server from the enable row", () => {
        const props = renderDetail();
        fireEvent.click(screen.getByRole("switch", { name: "启用此 MCP" }));
        expect(props.onToggleServer).toHaveBeenCalledWith(7, false);
    });

    it("should update tool enable and auto-run independently", () => {
        const props = renderDetail();
        fireEvent.click(screen.getByRole("switch", { name: "启用 web_search" }));
        expect(props.onUpdateTool).toHaveBeenCalledWith(11, false, false);
        fireEvent.click(screen.getByRole("switch", { name: "web_search 自动运行" }));
        expect(props.onUpdateTool).toHaveBeenCalledWith(11, true, true);
    });

    it("should refresh capabilities from the full-width button", () => {
        const props = renderDetail();
        fireEvent.click(screen.getByRole("button", { name: "测试连接并刷新能力" }));
        expect(props.onRefresh).toHaveBeenCalledWith(7);
    });

    it("should expand tool parameters when requested", () => {
        const props = renderDetail();
        fireEvent.click(screen.getByRole("button", { name: "参数与说明" }));
        expect(props.onToggleToolExpansion).toHaveBeenCalledWith(11);
    });

    it("should show the unexecutable notice when the server cannot run on this device", () => {
        renderDetail({ serverUnexecutable: true });
        expect(screen.getByText(/移动端不可执行/)).toBeTruthy();
    });
});
