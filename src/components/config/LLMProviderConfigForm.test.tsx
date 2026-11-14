import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import LLMProviderConfigForm from "./LLMProviderConfigForm";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), refresh: null as null | ((current: () => boolean) => Promise<void>), error: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: mocks.invoke }));
vi.mock("sonner", () => ({ toast: { error: mocks.error, success: vi.fn() } }));
vi.mock("@/hooks/useDataRefresh", () => ({ useDataRefresh: (options: { refresh: typeof mocks.refresh }) => { mocks.refresh = options.refresh; } }));
vi.mock("@/hooks/useCopilot", () => ({ useCopilot: () => ({ authInfo: {} }) }));
vi.mock("@/hooks/feature/useAcpEnvironment", () => ({ useAcpEnvironment: () => ({}) }));
vi.mock("./TagInputContainer", () => ({ default: () => null }));
vi.mock("./ModelSelectionDialog", () => ({ default: () => null }));
vi.mock("./ReadOnlyModelList", () => ({ default: () => null }));
vi.mock("../ConfigForm", async () => {
    const { Controller } = await import("react-hook-form");
    return { default: ({ useFormReturn, config }: { useFormReturn: any; config: any[] }) => {
        const apiKey = config.find(item => item.key === "api_key")?.config;
        return <div>
            <input aria-label="endpoint" {...useFormReturn.register("endpoint")} />
            {/* API Key 走真实的 customRender（保险箱引用字段），与 ConfigForm 的 custom 类型一致 */}
            <Controller control={useFormReturn.control} name="api_key" render={({ field }) => apiKey?.customRender(field)} />
            <span>{useFormReturn.formState.errors.endpoint?.message}</span><span>{config.find(item => item.key === "endpoint")?.config.hint}</span><span>{apiKey?.hint}</span>
        </div>;
    } };
});

const refresh = async () => { await act(async () => { await mocks.refresh!(() => true); }); };
const waitDebounce = async () => { await act(async () => { await new Promise(resolve => setTimeout(resolve, 80)); }); };

describe("LLM provider field autosave", () => {
    beforeEach(() => {
        mocks.invoke.mockReset(); mocks.error.mockReset();
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "get_llm_provider_config") return Promise.resolve([{ name: "endpoint", value: "remote" }, { name: "api_key", value: "old" }]);
            if (command === "get_llm_models") return Promise.resolve([]);
            return Promise.resolve();
        });
    });
    function mount() {
        render(<LLMProviderConfigForm id="1" index={0} apiType="openai_api" name="test" description="" isOffical={false} enabled onDelete={() => {}} onToggleEnabled={() => {}} />);
    }
    it("should show loading instead of an empty form when provider config has not loaded", async () => {
        mount();
        expect(screen.getByText("加载中...")).toBeInTheDocument();
        expect(screen.queryByLabelText("endpoint")).not.toBeInTheDocument();
        await refresh();
        expect(screen.queryByText("加载中...")).not.toBeInTheDocument();
        expect(screen.getByLabelText("endpoint")).toHaveValue("remote");
    });
    it("should leave loading when provider config read fails", async () => {
        mocks.invoke.mockImplementation((command: string) => command === "get_llm_provider_config" ? Promise.reject("db locked") : Promise.resolve([]));
        mount();
        await act(async () => { await expect(mocks.refresh!(() => true)).rejects.toBe("db locked"); });
        expect(screen.queryByText("加载中...")).not.toBeInTheDocument();
    });
    it("should show only errors when the endpoint changes", async () => {
        mocks.invoke.mockImplementation((command: string) => command === "get_llm_provider_config" || command === "get_llm_models" ? Promise.resolve([]) : Promise.resolve());
        mount(); await refresh();
        expect(screen.queryByText(/未填写|请求基址/)).not.toBeInTheDocument();
        expect(screen.queryByRole("alert")).not.toBeInTheDocument();
        const input = screen.getByLabelText("endpoint");
        fireEvent.change(input, { target: { value: "bad" } });
        expect(await screen.findByText(/接口地址无效/)).toBeInTheDocument();
        expect(screen.queryByText(/api.openai.com/)).not.toBeInTheDocument();
        fireEvent.change(input, { target: { value: "http://127.0.0.1:8080/v1" } });
        await waitFor(() => expect(screen.queryByRole("alert")).not.toBeInTheDocument());
        expect(screen.queryByText(/请求基址/)).not.toBeInTheDocument();
        expect(screen.queryByText(/接口地址无效/)).not.toBeInTheDocument();
        await waitDebounce();
    });
    it("saves the latest value for each field edited inside the debounce window", async () => {
        mount(); await refresh();
        fireEvent.change(screen.getByLabelText("endpoint"), { target: { value: "first" } });
        fireEvent.change(screen.getByLabelText("endpoint"), { target: { value: "latest" } });
        fireEvent.change(screen.getByLabelText("API Key"), { target: { value: "secret" } });
        await waitDebounce();
        const calls = mocks.invoke.mock.calls.filter(([command]) => command === "update_llm_provider_config");
        expect(calls).toHaveLength(2);
        expect(calls).toContainEqual(["update_llm_provider_config", { llmProviderId: "1", name: "endpoint", value: "latest" }]);
        expect(calls).toContainEqual(["update_llm_provider_config", { llmProviderId: "1", name: "api_key", value: "secret" }]);
    });
    it("preserves pending and failed input across refresh and accepts refresh after successful retry", async () => {
        let fail = true;
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "get_llm_provider_config") return Promise.resolve([{ name: "endpoint", value: "remote" }]);
            if (command === "get_llm_models") return Promise.resolve([]);
            return fail ? Promise.reject("database unavailable") : Promise.resolve();
        });
        mount(); await refresh();
        const input = screen.getByLabelText("endpoint");
        fireEvent.change(input, { target: { value: "local" } });
        await refresh(); expect(input).toHaveValue("local");
        await waitDebounce();
        await waitFor(() => expect(mocks.error).toHaveBeenCalledWith(expect.stringContaining("database unavailable")));
        await refresh(); expect(input).toHaveValue("local");
        fail = false; fireEvent.change(input, { target: { value: "retry" } });
        await waitDebounce(); await refresh(); expect(input).toHaveValue("remote");
    });
    it("preserves a pending edit when its save completes before an older refresh returns", async () => {
        let resolveRead!: (value: { name: string; value: string }[]) => void;
        let delayed = false;
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "get_llm_provider_config") {
                if (delayed) return new Promise(resolve => { resolveRead = resolve; });
                return Promise.resolve([{ name: "endpoint", value: "old" }]);
            }
            return command === "get_llm_models" ? Promise.resolve([]) : Promise.resolve();
        });
        mount(); await refresh();
        const input = screen.getByLabelText("endpoint");
        fireEvent.change(input, { target: { value: "new" } });
        delayed = true;
        let refreshPromise!: Promise<void>;
        act(() => { refreshPromise = mocks.refresh!(() => true); });
        await waitDebounce();
        expect(mocks.invoke).toHaveBeenCalledWith("update_llm_provider_config", { llmProviderId: "1", name: "endpoint", value: "new" });
        await act(async () => { resolveRead([{ name: "endpoint", value: "old" }]); await refreshPromise; });
        expect(input).toHaveValue("new");
    });
});

describe("LLM provider API key from the vault", () => {
    const vaultId = "0f8e2a52-6c1d-4b8a-9d3e-1a2b3c4d5e6f";
    beforeEach(() => {
        mocks.invoke.mockReset(); mocks.error.mockReset();
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "get_llm_provider_config") return Promise.resolve([{ name: "endpoint", value: "https://api.example.com/v1" }, { name: "api_key", value: "sk-old" }]);
            if (command === "get_llm_models") return Promise.resolve([]);
            if (command === "vault_capability") return Promise.resolve({ available: true });
            if (command === "vault_list") return Promise.resolve([{
                id: vaultId, key: "openai_key", name: "OpenAI Key", description: "", allowedSites: [], syncEnabled: true,
                status: "ready", createdAt: 1, updatedAt: 1, lastUsedAt: null,
            }]);
            return Promise.resolve();
        });
    });

    it("autosaves the chosen reference instead of a value and explains where it is sent", async () => {
        render(<LLMProviderConfigForm id="1" index={0} apiType="anthropic" name="test" description="" isOffical={false} enabled onDelete={() => {}} onToggleEnabled={() => {}} />);
        await refresh();
        fireEvent.click(await screen.findByRole("button", { name: "从凭据保险箱选择" }));
        expect(screen.getByText("保存配置后，这个值将被确认用于 https://api.example.com")).toBeInTheDocument();
        await act(async () => { fireEvent.click(screen.getByRole("button", { name: "确认" })); });
        await waitDebounce();
        expect(mocks.invoke).toHaveBeenCalledWith("update_llm_provider_config", { llmProviderId: "1", name: "api_key", value: `vault://${vaultId}` });
        expect(screen.getByText("OpenAI Key")).toBeInTheDocument();
        expect(screen.getByText(/只会发往已确认的网站；Claude Code 助手不能使用保险箱中的 Key/)).toBeInTheDocument();
    });
});
