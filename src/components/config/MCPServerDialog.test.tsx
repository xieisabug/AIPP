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
