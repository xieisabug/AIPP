import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import MCPServerDialog from "./MCPServerDialog";

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
});
