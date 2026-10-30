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
