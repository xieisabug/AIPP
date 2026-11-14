import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import MCPServerDialog from "./MCPServerDialog";
import type { MCPServer } from "../../data/MCP";

const platformState = vi.hoisted(() => ({ mobile: false }));
vi.mock("@/hooks/use-platform", () => ({
    useIsMobilePlatform: () => platformState.mobile,
}));
const toastMock = vi.hoisted(() => ({ error: vi.fn(), success: vi.fn(), warning: vi.fn() }));
vi.mock("sonner", () => ({ toast: toastMock }));

const stdioConfig = { name: "fs", transport_type: "stdio", command: "npx -y @modelcontextprotocol/server-filesystem /tmp" };

async function submit() {
    await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: "确定" }));
    });
}

describe("MCPServerDialog stdio gating", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        toastMock.error.mockClear();
        mockInvokeHandler("add_mcp_server", () => ({ id: 9, ...stdioConfig }));
        mockInvokeHandler("refresh_mcp_server_capabilities", () => [[], [], []]);
    });

    it("mobile: refuses to save an external stdio server before calling the backend", async () => {
        platformState.mobile = true;
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} initialConfig={stdioConfig} />);
        await submit();
        expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("MOBILE_UNSUPPORTED"));
        expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("add_mcp_server", expect.anything());
    });

    it("mobile: still allows aipp:* builtin commands registered as stdio", async () => {
        platformState.mobile = true;
        render(
            <MCPServerDialog
                isOpen
                onClose={() => undefined}
                onSubmit={() => undefined}
                initialConfig={{ name: "search", transport_type: "stdio", command: "aipp:search" }}
            />,
        );
        await submit();
        expect(toastMock.error).not.toHaveBeenCalledWith(expect.stringContaining("MOBILE_UNSUPPORTED"));
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("add_mcp_server", expect.anything());
    });

    it("desktop: saves the same stdio server normally", async () => {
        platformState.mobile = false;
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} initialConfig={stdioConfig} />);
        await submit();
        expect(toastMock.error).not.toHaveBeenCalledWith(expect.stringContaining("MOBILE_UNSUPPORTED"));
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("add_mcp_server", expect.anything());
    });

    it.each(["not a url", "ftp://example.com/mcp", "https://user:pass@example.com/mcp"])("rejects invalid HTTP address %s", async (url) => {
        platformState.mobile = true;
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} initialConfig={{ name: "server", transport_type: "http", url }} />);
        await submit();
        expect(screen.getByRole("alert")).toHaveTextContent("无效的 MCP 地址");
        expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("add_mcp_server", expect.anything());
    });

    it("retains edited values when saving fails", async () => {
        platformState.mobile = true;
        mockInvokeHandler("add_mcp_server", () => { throw new Error("save failed"); });
        const onSubmit = vi.fn();
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={onSubmit} initialConfig={{ name: "server", transport_type: "http", url: "https://example.com/mcp" }} />);
        fireEvent.change(screen.getByLabelText("描述"), { target: { value: "draft description" } });
        await submit();
        expect(screen.getByLabelText("描述")).toHaveValue("draft description");
        expect(screen.getByLabelText("URL *")).toHaveValue("https://example.com/mcp");
        expect(toastMock.error).toHaveBeenCalledWith(expect.stringContaining("save failed"));
        expect(onSubmit).not.toHaveBeenCalled();
    });

    it("keeps a synced external stdio server read-only on mobile", async () => {
        platformState.mobile = true;
        const server = { ...stdioConfig, id: 1, is_enabled: true, is_long_running: false } as MCPServer;
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} editingServer={server} />);
        expect(screen.getByRole("status")).toHaveTextContent("只读");
        expect(screen.getByLabelText("命令 *")).toBeDisabled();
        expect(screen.getByLabelText("命令 *")).toHaveValue(stdioConfig.command);
        expect(screen.getByRole("button", { name: "确定" })).toBeDisabled();
        expect(vi.mocked(invoke)).not.toHaveBeenCalledWith("update_mcp_server", expect.anything());
    });
});

describe("MCPServerDialog vault references", () => {
    const vaultId = "0f8e2a52-6c1d-4b8a-9d3e-1a2b3c4d5e6f";
    const vaultItem = {
        id: vaultId, key: "github_pat", name: "GitHub 个人令牌", description: "", allowedSites: [], syncEnabled: true,
        status: "ready", createdAt: 1, updatedAt: 1, lastUsedAt: null,
    };

    beforeEach(() => {
        platformState.mobile = false;
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        toastMock.error.mockClear();
        mockInvokeHandler("vault_capability", () => ({ available: true }));
        mockInvokeHandler("vault_list", () => [vaultItem]);
        mockInvokeHandler("add_mcp_server", () => 9);
        mockInvokeHandler("refresh_mcp_server_capabilities", () => [[], [], []]);
    });

    it("inserts an environment variable line that references the credential", async () => {
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} initialConfig={{ ...stdioConfig, environment_variables: "# 注释\nDEBUG=1" }} />);
        fireEvent.click(await screen.findByRole("button", { name: "插入保险箱凭据" }));
        expect(screen.getByLabelText("变量名")).toHaveAttribute("placeholder", "GITHUB_PAT");
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "确认" }));
        });
        const line = `GITHUB_PAT=vault://${vaultId}`;
        expect(screen.getByLabelText("环境变量")).toHaveValue(`# 注释\nDEBUG=1\n${line}`);
        expect(screen.getByText("GitHub 个人令牌")).toBeInTheDocument();
        expect(screen.getByText(/保存后，引用的保险箱凭据将被确认用于 本机程序 npx -y/)).toBeInTheDocument();
        await submit();
        expect(vi.mocked(invoke)).toHaveBeenCalledWith("add_mcp_server", {
            request: expect.objectContaining({ environment_variables: `# 注释\nDEBUG=1\n${line}` }),
        });
    });

    it("keeps the header prefix next to a referenced value and saves the reference", async () => {
        const headers = JSON.stringify({ Authorization: `Bearer vault://${vaultId}#token` });
        // 以打开状态直接挂载：初始化设置的请求头行不能被类型切换逻辑清掉
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} initialConfig={{ name: "gh", transport_type: "http", url: "https://mcp.example.com/mcp", headers }} />);
        fireEvent.click(screen.getByRole("button", { name: "高级设置" }));
        expect(await screen.findByText("GitHub 个人令牌")).toBeInTheDocument();
        expect(screen.getByLabelText("前缀")).toHaveValue("Bearer ");
        expect(screen.getByText("保存后，引用的保险箱凭据将被确认用于 https://mcp.example.com")).toBeInTheDocument();
        await submit();
        const call = vi.mocked(invoke).mock.calls.find(([command]) => command === "add_mcp_server");
        const request = (call?.[1] as { request: { headers: string } }).request;
        expect(JSON.parse(request.headers)).toEqual({ Authorization: `Bearer vault://${vaultId}#token` });
    });

    it("keeps header rows when switching to stdio and back", async () => {
        const headers = JSON.stringify({ Authorization: `Bearer vault://${vaultId}#token` });
        render(<MCPServerDialog isOpen onClose={() => undefined} onSubmit={() => undefined} initialConfig={{ name: "gh", transport_type: "http", url: "https://mcp.example.com/mcp", headers }} />);
        fireEvent.click(screen.getByTitle("HTTP"));
        fireEvent.click(screen.getByText("Stdio"));
        fireEvent.click(screen.getByTitle("Stdio"));
        fireEvent.click(screen.getByText("HTTP"));
        fireEvent.click(screen.getByRole("button", { name: "高级设置" }));
        expect(await screen.findByText("GitHub 个人令牌")).toBeInTheDocument();
        await submit();
        const call = vi.mocked(invoke).mock.calls.find(([command]) => command === "add_mcp_server");
        const request = (call?.[1] as { request: { headers: string; transport_type: string } }).request;
        expect(request.transport_type).toBe("http");
        expect(JSON.parse(request.headers)).toEqual({ Authorization: `Bearer vault://${vaultId}#token` });
    });
});
