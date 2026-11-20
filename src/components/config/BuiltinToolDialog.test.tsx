import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import BuiltinToolDialog from "./BuiltinToolDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const platform = vi.hoisted(() => ({ browser: false }));
vi.mock("@/lib/mobileUnsupported", async importOriginal => ({
    ...await importOriginal<typeof import("@/lib/mobileUnsupported")>(),
    useFeatureAvailableOnPlatform: () => platform.browser,
}));

describe("search service drives which settings are shown", () => {
    beforeEach(() => { platform.browser = true; });
    afterEach(() => { platform.browser = false; });
    const template = {
        id: "search", command: "aipp:search", name: "搜索工具", required_envs: [
            { key: "HTTP_SEARCH_PROVIDER", label: "搜索服务", field_type: "select", required: true, default_value: "local", options: [
                { label: "本地搜索（浏览器）", value: "local" }, { label: "DeepSeek", value: "deepseek" }, { label: "Tavily", value: "tavily" },
            ] },
            { key: "SEARCH_ENGINE", label: "搜索引擎", field_type: "select", default_value: "google", options: [
                { label: "Google", value: "google" }, { label: "Kagi", value: "kagi" },
            ] },
            { key: "KAGI_SESSION_URL", label: "Kagi 会话链接", field_type: "text" },
            { key: "HTTP_SEARCH_ENDPOINT", label: "搜索 API 地址", field_type: "text" },
            { key: "HTTP_SEARCH_API_KEY", label: "搜索 API Key", field_type: "password" },
            { key: "FETCH_PROVIDER", label: "网页抓取方式", field_type: "select", default_value: "local", options: [
                { label: "本机抓取", value: "local" }, { label: "DeepSeek Fetch", value: "deepseek" },
            ] },
            { key: "PROXY_SERVER", label: "代理服务器", field_type: "text" },
        ],
    };

    async function open(envText: string) {
        vi.mocked(invoke).mockImplementation(async (command: string) => command === "list_aipp_builtin_templates" ? [template] : { available: false });
        render(<BuiltinToolDialog isOpen editing initialCommand="aipp:search" initialEnvText={envText}
            onClose={vi.fn()} onSubmit={vi.fn()} onEnvChange={vi.fn()} />);
        await screen.findByText("搜索服务");
    }

    it("local search shows the engine and browser settings but no API fields", async () => {
        await open("HTTP_SEARCH_PROVIDER=disabled\nSEARCH_ENGINE=google");
        expect(screen.getByText("搜索引擎")).toBeInTheDocument();
        expect(screen.queryByText("Kagi 会话链接")).toBeNull();
        expect(screen.queryByText("搜索 API Key")).toBeNull();
        expect(screen.queryByText("网页抓取方式")).toBeNull();
        expect(screen.getByText("浏览器设置")).toBeInTheDocument();
        expect(screen.getByText("代理服务器")).toBeInTheDocument();
    });

    it("the Kagi link appears only for the Kagi engine", async () => {
        await open("HTTP_SEARCH_PROVIDER=local\nSEARCH_ENGINE=kagi");
        expect(screen.getByText("Kagi 会话链接")).toBeInTheDocument();
    });

    it("an HTTP service hides the engine and keeps browser settings collapsed for local fetch", async () => {
        await open("HTTP_SEARCH_PROVIDER=tavily\nSEARCH_ENGINE=kagi\nHTTP_SEARCH_API_KEY=k");
        expect(screen.queryByText("搜索引擎")).toBeNull();
        expect(screen.queryByText("Kagi 会话链接")).toBeNull();
        expect(screen.getByText("搜索 API Key")).toBeInTheDocument();
        expect(screen.queryByText("网页抓取方式")).toBeNull();
        expect(screen.getByText("浏览器设置")).toBeInTheDocument();
        expect(screen.getByText(/fetch_url 本机抓取网页时使用/)).toBeInTheDocument();
        expect(screen.queryByText("代理服务器")).toBeNull();
    });

    it("DeepSeek with DeepSeek Fetch needs no browser settings at all", async () => {
        await open("HTTP_SEARCH_PROVIDER=deepseek\nFETCH_PROVIDER=deepseek\nHTTP_SEARCH_API_KEY=k");
        expect(screen.getByText("网页抓取方式")).toBeInTheDocument();
        expect(screen.queryByText("浏览器设置")).toBeNull();
    });
});

describe("HTTP search configuration", () => {
    it("should retain HTTP settings and mask credentials when editing on mobile", async () => {
        vi.mocked(invoke).mockResolvedValue([{
            id: "search", command: "aipp:search", name: "搜索工具", required_envs: [
                { key: "HTTP_SEARCH_API_KEY", label: "HTTP 搜索 API Key", field_type: "password" },
                { key: "HTTP_SEARCH_ENDPOINT", label: "HTTP 搜索 API 地址", field_type: "text" },
                { key: "SEARCH_ENGINE", label: "浏览器搜索引擎", field_type: "text" },
            ],
        }]);
        const onEnvChange = vi.fn();
        render(<BuiltinToolDialog isOpen editing initialCommand="aipp:search"
            initialEnvText={"HTTP_SEARCH_PROVIDER=tavily\nHTTP_SEARCH_API_KEY=example-key\nSEARCH_ENGINE=bing"}
            onClose={vi.fn()} onSubmit={vi.fn()} onEnvChange={onEnvChange} />);
        const key = await screen.findByLabelText("HTTP 搜索 API Key");
        expect(key).toHaveAttribute("type", "password");
        await waitFor(() => expect(key).toHaveValue("example-key"));
        expect(screen.queryByLabelText("浏览器搜索引擎")).toBeNull();
        fireEvent.change(screen.getByLabelText("HTTP 搜索 API 地址"), { target: { value: "https://search.example.com/search" } });
        await waitFor(() => expect(onEnvChange).toHaveBeenLastCalledWith(expect.stringContaining("HTTP_SEARCH_ENDPOINT=https://search.example.com/search")));
        expect(onEnvChange.mock.lastCall?.[0]).toContain("HTTP_SEARCH_PROVIDER=tavily");
        expect(onEnvChange.mock.lastCall?.[0]).toContain("SEARCH_ENGINE=bing");
    });

    it("should show a vault reference for the search API key as the entry, not the reference", async () => {
        const ref = "vault://3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b";
        vi.mocked(invoke).mockImplementation(async (command: string) => {
            if (command === "vault_capability") return { available: true };
            if (command === "vault_list") return [{ id: "3f2a9c1e-0b7d-4e5f-9a12-6c8d0e1f2a3b", key: "deepseek_api_key", name: "DeepSeek · API Key" }];
            return [{
                id: "search", command: "aipp:search", name: "搜索工具", required_envs: [
                    { key: "HTTP_SEARCH_API_KEY", label: "HTTP 搜索 API Key", field_type: "password" },
                ],
            }];
        });
        const onEnvChange = vi.fn();
        render(<BuiltinToolDialog isOpen editing initialCommand="aipp:search"
            initialEnvText={`HTTP_SEARCH_PROVIDER=deepseek
HTTP_SEARCH_API_KEY=${ref}`}
            onClose={vi.fn()} onSubmit={vi.fn()} onEnvChange={onEnvChange} />);
        expect(await screen.findByText("DeepSeek · API Key")).toBeInTheDocument();
        expect(screen.queryByDisplayValue(ref)).toBeNull();
        await waitFor(() => expect(onEnvChange).toHaveBeenLastCalledWith(expect.stringContaining(`HTTP_SEARCH_API_KEY=${ref}`)));
    });
});
