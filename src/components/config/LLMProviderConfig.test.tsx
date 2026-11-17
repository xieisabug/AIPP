import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "../../__tests__/mocks/tauri";
import LLMProviderConfig from "./LLMProviderConfig";

const platformState = vi.hoisted(() => ({ value: "windows" }));

vi.mock("@/hooks/use-platform", () => ({
    usePlatform: () => platformState.value,
    useIsMobilePlatform: () => platformState.value === "android" || platformState.value === "ios",
}));

// 详情表单与本测试无关，避免其加载配置的副作用
vi.mock("./LLMProviderConfigForm", () => ({
    default: ({ name }: { name: string }) => <div data-testid="provider-form">{name}</div>,
}));

const openai = { id: 1, name: "OpenAI", api_type: "openai_api", description: "", is_official: true, is_enabled: true };

function mockBackend(initial: typeof openai[]) {
    let providers = [...initial];
    mockInvokeHandler("get_llm_providers", () => providers);
    mockInvokeHandler("vault_capability", () => ({ available: false }));
    mockInvokeHandler("add_llm_provider", (args) => {
        const created = { id: 7, name: String(args?.name), api_type: String(args?.apiType), description: "", is_official: false, is_enabled: false };
        providers = [...providers, created];
        return created.id;
    });
    mockInvokeHandler("update_llm_provider", (args) => {
        providers = providers.map((p) => p.id === args?.id ? { ...p, name: String(args?.name), is_enabled: Boolean(args?.isEnabled) } : p);
    });
}

describe("LLMProviderConfig new provider wizard", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
        platformState.value = "windows";
    });

    it("should open the type grid from the add button", async () => {
        mockBackend([openai]);
        render(<LLMProviderConfig />);
        fireEvent.click(await screen.findByRole("button", { name: "添加提供商" }));
        expect(await screen.findByTestId("wizard-step")).toHaveTextContent("选择 API 类型");
        expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: /Claude Code/ })).toBeInTheDocument();
    });

    it("should keep the wizard open when its draft turns the empty page into a list", async () => {
        mockBackend([]);
        mockInvokeHandler("update_llm_provider_config", () => undefined);
        mockInvokeHandler("preview_model_list", () => ({ available_models: [], missing_models: [] }));
        render(<LLMProviderConfig />);
        fireEvent.click(await screen.findByRole("button", { name: /添加第一个提供商/ }));
        fireEvent.click(await screen.findByRole("button", { name: /DeepSeek/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        expect(await screen.findByText(/接口没有返回模型/)).toBeInTheDocument();

        // 后端广播变更：列表刷新后出现草稿，页面从空状态切到列表布局
        await act(async () => { await emit("llm_provider_changed"); });
        await waitFor(() => expect(screen.getByTestId("provider-form")).toHaveTextContent("DeepSeek API"));
        expect(screen.getByTestId("wizard-step")).toHaveTextContent("3/3");
        expect(invoke).not.toHaveBeenCalledWith("delete_llm_provider", expect.anything());

        fireEvent.change(screen.getByLabelText("自定义模型"), { target: { value: "deepseek-chat" } });
        mockInvokeHandler("add_llm_model", () => ({}));
        fireEvent.click(screen.getByRole("button", { name: "添加" }));
        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(screen.queryByTestId("wizard-step")).not.toBeInTheDocument());
        expect(invoke).toHaveBeenCalledWith("update_llm_provider", expect.objectContaining({ id: 7, isEnabled: true }));
        expect(invoke).not.toHaveBeenCalledWith("delete_llm_provider", expect.anything());
    });

    it("should hide local agent types on mobile", async () => {
        platformState.value = "android";
        mockBackend([openai]);
        render(<LLMProviderConfig />);
        fireEvent.click((await screen.findAllByRole("button", { name: "添加提供商" }))[0]);
        expect(await screen.findByRole("button", { name: /OpenAI API/ })).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: /Claude Code/ })).not.toBeInTheDocument();
        expect(screen.queryByText("本地智能体")).not.toBeInTheDocument();
    });
});
