import { ComposerPermissionSlot } from "./conversation/ComposerPermissionSlot";
import { fireEvent, render, screen } from "@testing-library/react";
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
    it("should move the same request between the composer and dialog when switching conversations", () => {
        const decide = vi.fn();
        const view = (id: number) => <>
            <ComposerPermissionSlot conversationIds={[id]} placement="bottom" />
            <AcpPermissionDialog request={request} isOpen onDecision={decide} />
        </>;
        const { rerender } = render(view(1));
        expect(screen.queryByRole("alertdialog")).toBeNull();
        const summary = screen.getByText("Pi 工具权限请求").closest("summary")!;
        fireEvent.click(summary);
        expect(summary.parentElement).not.toHaveAttribute("open");
        rerender(view(2));
        expect(screen.getByRole("alertdialog")).toHaveTextContent("允许 Pi 使用 bash");
        rerender(view(1));
        expect(screen.queryByRole("alertdialog")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "本次允许" }));
        expect(decide).toHaveBeenCalledWith(request.request_id, "allow", false);
        expect(decide).toHaveBeenCalledTimes(1);
    });

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
        for (const text of ["lookup", "catalog", "货号", "0", "查找库存"]) expect(screen.getByText(text)).toBeVisible();
    });

    it("should not show a review note for requests that were never reviewed", () => {
        render(<AcpPermissionDialog request={request} isOpen onDecision={vi.fn()} />);
        expect(screen.queryByRole("status")).not.toBeInTheDocument();
    });

    it("should show the Antigravity tool and arguments and distinguish rejection from cancellation", () => {
        const decide = vi.fn();
        const cancel = "__aipp_cancel_permission__";
        render(<AcpPermissionDialog isOpen onDecision={decide} request={{
            ...request, agent_kind: "antigravity", title: "Run mcp_aipp_aipp_t62_search_bot_history",
            parameters: JSON.stringify({ bot_id: 43, query: "5b121932", toolSummary: "查找回归结果" }),
            options: [...request.options, { option_id: cancel, name: "拒绝", kind: "reject_once" }],
        }} />);
        for (const label of ["搜索管家聊天记录（含归档会话）", "助手编号", "43", "搜索词", "5b121932", "用途", "查找回归结果"]) {
            expect(screen.getByText(label)).toBeVisible();
        }
        expect(screen.getAllByRole("button")).toHaveLength(3);
        fireEvent.click(screen.getByRole("button", { name: "本次拒绝" }));
        fireEvent.click(screen.getByRole("button", { name: "取消请求" }));
        expect(decide.mock.calls).toEqual([[request.request_id, "deny", false], [request.request_id, cancel, false]]);
    });

    it("should preserve unfamiliar tool names and unstructured Antigravity parameters", () => {
        render(<AcpPermissionDialog isOpen onDecision={vi.fn()} request={{
            ...request, agent_kind: "antigravity", title: "Run mcp_aipp_aipp_t91_custom_tool", parameters: "plain text",
        }} />);
        expect(screen.getByText("custom_tool")).toBeVisible();
        expect(screen.getByText("plain text")).toBeVisible();
    });

    it("should show the wrapped tool, purpose and readable command when Antigravity calls a dynamic MCP tool", () => {
        const command = "uv.exe run python -c \"\nimport sqlite3\nprint('C:\\Users')\n\"";
        render(<AcpPermissionDialog isOpen onDecision={vi.fn()} request={{
            ...request, agent_kind: "antigravity", title: "Run mcp_aipp_aipp_t62_call_mcp_tool",
            parameters: JSON.stringify({ server_name: "操作工具", tool_name: "execute_bash", toolSummary: "Inspect real DeepSeek call logs",
                parameters: { command, description: "Inspect real DeepSeek LLM logs" } }),
        }} />);
        const rows = [...document.querySelectorAll("dt")].map(dt => dt.textContent);
        expect(rows).toEqual(["工具", "用途", "命令", "说明"]);
        expect(screen.getByText("运行命令")).toBeVisible();
        expect(screen.getByText("操作工具 · execute_bash")).toBeVisible();
        expect(screen.getByText("Inspect real DeepSeek call logs")).toBeVisible();
        // The command keeps real line breaks and single backslashes instead of JSON escapes.
        expect(screen.getByText((_, el) => el?.tagName === "PRE" && el.textContent === command)).toBeVisible();
        expect(screen.queryByText("call_mcp_tool")).toBeNull();
        expect(screen.getByText("请求详情").closest("details")).not.toHaveAttribute("open");
    });

    it("should show the Claude Code tool and its input instead of the raw control request", () => {
        const parameters = JSON.stringify({ subtype: "can_use_tool", tool_name: "Bash", input: { command: "git status", description: "查看改动" } });
        render(<AcpPermissionDialog isOpen onDecision={vi.fn()} request={{ ...request, agent_kind: "claude_sdk", title: "允许 Claude Code 使用 Bash", parameters }} />);
        expect(screen.getByText("运行命令")).toBeVisible();
        expect(screen.getByText("查看改动")).toBeVisible();
        expect(screen.getByText("git status")).toBeVisible();
        expect(screen.queryByText("can_use_tool")).toBeNull();
    });

    it("should name the assistant that asks for permission in the collapsed title", () => {
        render(<>
            <ComposerPermissionSlot conversationIds={[1]} placement="bottom" />
            <AcpPermissionDialog request={{ ...request, agent_kind: "antigravity", assistant_name: "leader前端开发" }} isOpen onDecision={vi.fn()} />
        </>);
        expect(screen.getByText("Antigravity 工具权限请求", { exact: false }).closest("summary")).toHaveTextContent("leader前端开发·Antigravity 工具权限请求");
    });
});
