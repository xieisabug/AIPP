import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { invoke } from "@tauri-apps/api/core";
import BuiltinToolDialog from "./BuiltinToolDialog";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@/lib/mobileUnsupported", async importOriginal => ({
    ...await importOriginal<typeof import("@/lib/mobileUnsupported")>(),
    useFeatureAvailableOnPlatform: () => false,
}));

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
});
