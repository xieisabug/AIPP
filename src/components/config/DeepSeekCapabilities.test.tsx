import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { useState } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "../../__tests__/mocks/tauri";
import DeepSeekCapabilities from "./DeepSeekCapabilities";

const REF = "vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b";

function server(env: string, enabled = true) {
    return {
        id: 5, name: "搜索工具", description: null, transport_type: "stdio", command: "aipp:search", environment_variables: env,
        url: null, timeout: 60000, is_long_running: false, is_enabled: enabled, is_builtin: true, is_deletable: true, created_time: "",
    };
}

function renderBlock(apiKey: string, servers: unknown[], vaultAvailable = true, initialShowBalanceInChat = false) {
    let current = servers;
    mockInvokeHandler("get_mcp_servers", () => current);
    mockInvokeHandler("update_mcp_server", (args) => {
        const request = (args as { request: { environment_variables: string } }).request;
        current = [server(request.environment_variables)];
    });
    mockInvokeHandler("vault_create", () => ({ id: "3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b" }));
    mockInvokeHandler("update_llm_provider_config", () => undefined);
    mockInvokeHandler("get_deepseek_balance", () => ({ available: true, currency: "CNY", total: "19.00" }));
    const onKeyShared = vi.fn();
    const onShowBalanceInChatChange = vi.fn();
    // 与服务商表单一致：Key 存入保险箱后表单改为引用，再传回本组件
    function Harness() {
        const [key, setKey] = useState(apiKey);
        const [showBalanceInChat, setShowBalanceInChat] = useState(initialShowBalanceInChat);
        return (
            <DeepSeekCapabilities
                providerId={42}
                providerName="DeepSeek"
                apiKey={key}
                endpoint="https://api.deepseek.com/"
                vault={{ available: vaultAvailable, items: [] }}
                onKeyShared={(value) => { onKeyShared(value); setKey(value); }}
                showBalanceInChat={showBalanceInChat}
                onShowBalanceInChatChange={(enabled) => {
                    onShowBalanceInChatChange(enabled);
                    setShowBalanceInChat(enabled);
                }}
            />
        );
    }
    render(<Harness />);
    return { onKeyShared, onShowBalanceInChatChange };
}

describe("DeepSeekCapabilities", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
    });

    it("shows that search and fetch use this key", async () => {
        renderBlock(REF, [server(`HTTP_SEARCH_PROVIDER=deepseek\nHTTP_SEARCH_API_KEY=${REF}\nFETCH_PROVIDER=deepseek`)]);
        expect(await screen.findByText("联网搜索与网页抓取使用此 Key")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: /启用|改用/ })).toBeNull();
    });

    it("offers to switch from another search provider and shares the key through the vault", async () => {
        const { onKeyShared } = renderBlock("sk-plain", [server("HTTP_SEARCH_PROVIDER=tavily\nHTTP_SEARCH_API_KEY=tvly")]);
        fireEvent.click(await screen.findByRole("button", { name: "改用 DeepSeek" }));
        await waitFor(() => expect(onKeyShared).toHaveBeenCalledWith(REF));
        expect(invoke).toHaveBeenCalledWith("update_llm_provider_config", { llmProviderId: 42, name: "api_key", value: REF });
        const update = invoke.mock.calls.find(([command]) => command === "update_mcp_server")?.[1] as { request: { environment_variables: string } };
        expect(update.request.environment_variables).toContain(`HTTP_SEARCH_API_KEY=${REF}`);
        expect(update.request.environment_variables).not.toContain("tvly");
        expect(await screen.findByText("联网搜索与网页抓取使用此 Key")).toBeInTheDocument();
    });

    it("asks for a key before anything can be enabled", async () => {
        renderBlock("", []);
        expect(await screen.findByText("填写 API Key 后可用于内置搜索")).toBeInTheDocument();
        expect(screen.queryByRole("button")).toBeNull();
        expect(screen.queryByTestId("deepseek-balance")).toBeNull();
        expect(invoke).not.toHaveBeenCalledWith("get_deepseek_balance", expect.anything());
    });

    it("shows the account balance, toggles chat display, and refreshes on demand", async () => {
        const { onShowBalanceInChatChange } = renderBlock(REF, []);
        expect(await screen.findByTestId("deepseek-balance")).toHaveTextContent("¥19.00");
        const chatToggle = screen.getByRole("switch", { name: "在聊天窗口展示余额" });
        expect(chatToggle).toHaveAttribute("aria-checked", "false");
        fireEvent.click(chatToggle);
        expect(onShowBalanceInChatChange).toHaveBeenCalledWith(true);
        expect(chatToggle).toHaveAttribute("aria-checked", "true");
        mockInvokeHandler("get_deepseek_balance", () => ({ available: false, currency: "CNY", total: "0.00" }));
        fireEvent.click(screen.getByRole("button", { name: "刷新余额" }));
        await waitFor(() => expect(screen.getByTestId("deepseek-balance")).toHaveTextContent("¥0.00（余额不足，暂不可调用）"));
        expect(invoke).toHaveBeenCalledWith("get_deepseek_balance", { llmProviderId: 42 });
    });

    it("re-enables a disabled search server and flags a different DeepSeek key", async () => {
        renderBlock(REF, [server(`HTTP_SEARCH_PROVIDER=deepseek\nHTTP_SEARCH_API_KEY=${REF}`, false)]);
        expect(await screen.findByRole("button", { name: "启用" })).toBeInTheDocument();
        expect(screen.getByTestId("deepseek-search-status")).toHaveTextContent("内置搜索已停用");
    });
});
