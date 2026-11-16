import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AcpPermissionDialog } from "./OperationPermissionDialog";

const request = {
    request_id: "pi:1:run:2:req",
    conversation_id: 1,
    agent_kind: "pi",
    tool_call_id: "req",
    title: "允许 Pi 使用 bash",
    kind: "native_agent_permission",
    parameters: "{\"command\":\"rm -rf build\"}",
    options: [{ option_id: "allow", name: "本次允许", kind: "allow_once" }, { option_id: "deny", name: "拒绝", kind: "reject_once" }],
};

describe("AcpPermissionDialog", () => {
    it("should explain why AIPP auto-review handed the request to the user", () => {
        render(<AcpPermissionDialog request={{ ...request, review: { verdict: "risky", reason: "自动审核认为需要人工确认：递归删除目录" } }} isOpen onDecision={vi.fn()} />);
        expect(screen.getByRole("status")).toHaveTextContent("递归删除目录");
        expect(screen.getByText("Pi 工具权限请求")).toBeInTheDocument();
    });

    it("should show the command and purpose when Codex asks to start a local server", () => {
        const parameters = JSON.stringify({ reason: "启动本地服务以验证布局", command: "pnpm dev", cwd: "/project/frontend" });
        render(<AcpPermissionDialog request={{ ...request, agent_kind: "codex_app_server", parameters }} isOpen onDecision={vi.fn()} />);
        expect(screen.getByText("启动本地服务以验证布局")).toBeVisible();
        expect(screen.getByText("pnpm dev")).toBeVisible();
        expect(screen.getByText("/project/frontend")).toBeVisible();
        expect(screen.getByText("原始请求").closest("details")).not.toHaveAttribute("open");
    });

    it("should show the search scope, actual arguments and timeout when approving a Butler history search", () => {
        const parameters = JSON.stringify({
            serverName: "aipp", mode: "form", message: "Allow the aipp MCP server to run tool?",
            _meta: { codex_approval_kind: "mcp_tool_call", tool_name: "aipp_t62_search_bot_history",
                tool_params: { bot_id: 38, query: "325" },
                tool_params_display: [{ name: "query", display_name: "query", value: "truncated" }],
                tool_description: "Search archived conversations.", persist: ["session", "always"] },
        });
        const review = { verdict: "error", reason: "自动审核输出请求失败 (call_id=0, model=deepseek-v4-flash, provider_id=41): 自动审核模型超时（20秒）" };
        render(<AcpPermissionDialog request={{ ...request, parameters, review, agent_kind: "codex_app_server" }} isOpen onDecision={vi.fn()} />);
        for (const text of ["搜索管家聊天记录（含归档会话）", "助手编号", "38", "搜索词", "325"]) {
            expect(screen.getByText(text)).toBeVisible();
        }
        expect(screen.getByText("自动审核在 20 秒内没有返回结果，请你决定是否允许。")).toBeVisible();
        expect(screen.getByText(review.reason).closest("details")).not.toHaveAttribute("open");
        expect(screen.getByText("原始请求").closest("details")).not.toHaveAttribute("open");
    });

    it("should use the supplied labels and values when another MCP tool provides display parameters only", () => {
        const parameters = JSON.stringify({ serverName: "catalog", _meta: {
            codex_approval_kind: "mcp_tool_call", tool_name: "lookup", tool_description: "查找库存",
            tool_params_display: [{ name: "item_id", display_name: "货号", value: 0 }],
        } });
        render(<AcpPermissionDialog request={{ ...request, parameters, agent_kind: "codex_app_server" }} isOpen onDecision={vi.fn()} />);
        for (const text of ["catalog / lookup", "货号", "0", "查找库存"]) expect(screen.getByText(text)).toBeVisible();
    });

    it("should not show a review note for requests that were never reviewed", () => {
        render(<AcpPermissionDialog request={request} isOpen onDecision={vi.fn()} />);
        expect(screen.queryByRole("status")).not.toBeInTheDocument();
    });
});
