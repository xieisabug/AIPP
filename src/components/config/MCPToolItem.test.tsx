import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import MCPToolItem from "./MCPToolItem";
import { MOBILE_UNEXECUTABLE_LABEL } from "../../lib/mobileUnsupported";

const tool: any = {
    id: 1, server_id: 3, tool_name: "search_web", tool_description: "搜索网络内容",
    is_enabled: true, is_auto_run: false, parameters: "{}",
};
const noop = () => undefined;

describe("MCPToolItem mobile-unexecutable label", () => {
    it("keeps the tool name and puts the label on its own line", () => {
        render(<MCPToolItem tool={tool} isExpanded={false} onToggleExpansion={noop} onUpdateTool={noop} truncateText={(t) => t} mobileUnexecutable />);
        expect(screen.getByText("search_web")).toBeTruthy();
        const label = screen.getByText(MOBILE_UNEXECUTABLE_LABEL);
        expect(label.closest("[data-aipp-slot=mcp-tool-mobile-unexecutable]")).toBeTruthy();
        const nameRow = screen.getByText("search_web").parentElement!;
        expect(nameRow.contains(label)).toBe(false);
    });

    it("renders no label by default (desktop)", () => {
        render(<MCPToolItem tool={tool} isExpanded={false} onToggleExpansion={noop} onUpdateTool={noop} truncateText={(t) => t} />);
        expect(screen.queryByText(MOBILE_UNEXECUTABLE_LABEL)).toBeNull();
    });
});

describe("MCPToolItem narrow layout", () => {
    it("keeps description full width in a vertical card header", () => {
        const long = "这是一段很长的工具说明，用于验证窄屏下不会被挤成每行两三个字。";
        render(
            <MCPToolItem
                tool={{ ...tool, tool_description: long }}
                isExpanded={false}
                onToggleExpansion={noop}
                onUpdateTool={noop}
                truncateText={(t) => t}
            />,
        );
        const item = screen.getByText(long).closest("[data-aipp-slot=mcp-tool-item]");
        expect(item).toBeTruthy();
        expect(screen.getByText("启用")).toBeTruthy();
        expect(screen.getByText("自动运行")).toBeTruthy();
    });
});
