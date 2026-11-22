import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import type { AssistantDetail } from "@/data/Assistant";
import AddAssistantDialog from "./AddAssistantDialog";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false, usePlatform: () => "windows" }));
vi.mock("@/hooks/useFilteredModels", () => ({ useFilteredModels: (type: number) => ({
    models: type === 4
        ? [{ name: "Agent provider", code: "acp", llm_provider_id: 20 }]
        : [{ name: "Test model", code: "test", llm_provider_id: 10 }, { name: "Other model", code: "other", llm_provider_id: 10 }],
    loading: false, error: null,
}) }));
// 字段展示组件各自读写后端，这里只验证向导把它们放进了第三步
vi.mock("@/components/config/AssistantMCPFieldDisplay", () => ({ default: () => <div>MCP 字段</div> }));
vi.mock("@/components/config/AssistantSkillsFieldDisplay", () => ({ default: () => <div>Skills 字段</div> }));
vi.mock("@/components/config/AssistantWorkspaceFieldDisplay", () => ({ default: () => <div>工作区字段</div> }));

const detail = (id: number, type: number, extra: Partial<AssistantDetail> = {}): AssistantDetail => ({
    assistant: { id, name: "New", description: "", assistant_type: type, is_addition: false, created_time: "" },
    // 与后端一致：没有传入提示词时写入默认提示词
    prompts: [{ id: 1, assistant_id: id, prompt: "You are a helpful assistant.", created_time: "" }],
    model: [{ id: 3, assistant_id: id, model_code: type === 4 ? "" : "test", provider_id: type === 4 ? 20 : 10, alias: "" }],
    model_configs: type === 4 ? [] : [{ id: 5, assistant_id: id, name: "temperature", value: "0.7", value_type: "float", assistant_model_id: 3 }],
    prompt_params: [], mcp_configs: [], mcp_tool_configs: [],
    ...extra,
});

const types = [{ code: 0, name: "普通" }, { code: 4, name: "Agent" }];

async function fillBasics(user: ReturnType<typeof userEvent.setup>, { type, model }: { type?: string; model: string }) {
    await user.click(screen.getByRole("button", { name: "add" }));
    fireEvent.change(screen.getByLabelText("名称"), { target: { value: "New" } });
    if (type) {
        await user.click(screen.getAllByRole("combobox")[0]);
        await user.click(screen.getByRole("option", { name: type }));
    }
    await user.click(screen.getAllByRole("combobox")[1]);
    await user.click(screen.getByRole("option", { name: model }));
    await user.click(screen.getByRole("button", { name: "下一步" }));
}

describe("AddAssistantDialog wizard", () => {
    let saveConfig: ReturnType<typeof vi.fn<(args?: Record<string, unknown>) => unknown>>;
    beforeEach(() => {
        mockInvokeHandler("get_all_feature_config", () => [{ id: 1, feature_code: "butler_appearance", key: "butler_appearance_3", value: "{}" }]);
        saveConfig = vi.fn<(args?: Record<string, unknown>) => unknown>();
        mockInvokeHandler("save_feature_config", saveConfig);
        mockInvokeHandler("get_filtered_providers", () => []);
    });
    afterEach(clearAllMockHandlers);

    it("should require a model before leaving the basic step", async () => {
        const user = userEvent.setup();
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await user.click(screen.getByRole("button", { name: "add" }));
        fireEvent.change(screen.getByLabelText("名称"), { target: { value: "New" } });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        expect(await screen.findByText("请选择模型")).toBeInTheDocument();
        expect(screen.getByTestId("wizard-step")).toHaveTextContent("1/3");
    });

    it("should create the assistant with its appearance and save advanced settings on finish", async () => {
        const user = userEvent.setup();
        const add = vi.fn().mockReturnValue(detail(9, 0));
        mockInvokeHandler("add_assistant", add);
        const save = vi.fn();
        mockInvokeHandler("save_assistant", save);
        mockInvokeHandler("get_assistant", () => detail(9, 0));
        const added = vi.fn();
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={added} triggerButtonProps={{ "aria-label": "add" }} />);

        await fillBasics(user, { model: "Test model" });
        expect(screen.getByTestId("wizard-step")).toHaveTextContent("2/3");
        expect(add).not.toHaveBeenCalled();
        await user.click(screen.getByRole("button", { name: "头部：贝雷帽" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));

        // 提示词留空时写入默认提示词
        await waitFor(() => expect(add).toHaveBeenCalledWith({ name: "New", description: "", assistantType: 0, modelSelection: "test%%10", prompt: "You are a helpful assistant." }));
        // 形象与编辑助手共用 butler_appearance 配置，保留其他助手已有的形象。
        await waitFor(() => expect(saveConfig).toHaveBeenCalledOnce());
        const config = (saveConfig.mock.calls[0][0] as { config: Record<string, string> }).config;
        expect(config.butler_appearance_3).toBeUndefined(); // Saving one assistant leaves other devices’ settings untouched.
        expect(JSON.parse(config.butler_appearance_9)).toMatchObject({ accessories: { head: "beret" }, motion: true });

        const advanced = await screen.findByRole("region", { name: "高级配置" });
        expect(within(advanced).getByText("MCP 字段")).toBeInTheDocument();
        const temperature = within(advanced).getByDisplayValue("0.7");
        fireEvent.change(temperature, { target: { value: "0.2" } });
        await user.click(screen.getByRole("button", { name: "完成" }));

        await waitFor(() => expect(save).toHaveBeenCalledOnce());
        const saved = save.mock.calls[0][0].assistantDetail as AssistantDetail;
        expect(saved.assistant).toMatchObject({ id: 9, name: "New" });
        expect(saved.model_configs.find(item => item.name === "temperature")).toMatchObject({ id: 5, value: "0.2" });
        await waitFor(() => expect(added).toHaveBeenCalledWith(expect.objectContaining({ assistant: expect.objectContaining({ id: 9 }) })));
    });

    it("should not rewrite the assistant when finishing without changes", async () => {
        const user = userEvent.setup();
        mockInvokeHandler("add_assistant", () => detail(9, 0));
        const save = vi.fn();
        mockInvokeHandler("save_assistant", save);
        mockInvokeHandler("get_assistant", () => detail(9, 0));
        const added = vi.fn();
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={added} triggerButtonProps={{ "aria-label": "add" }} />);
        await fillBasics(user, { model: "Test model" });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("region", { name: "高级配置" });
        await user.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(added).toHaveBeenCalledOnce());
        expect(save).not.toHaveBeenCalled();
    });

    it("should delete the created assistant when the wizard is closed before finishing", async () => {
        const user = userEvent.setup();
        mockInvokeHandler("add_assistant", () => detail(9, 0));
        const remove = vi.fn();
        mockInvokeHandler("delete_assistant", remove);
        const added = vi.fn();
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={added} triggerButtonProps={{ "aria-label": "add" }} />);
        await fillBasics(user, { model: "Test model" });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("region", { name: "高级配置" });
        await user.click(screen.getByRole("button", { name: "关闭" }));
        await waitFor(() => expect(remove).toHaveBeenCalledWith({ assistantId: 9 }));
        expect(added).not.toHaveBeenCalled();
    });

    it("should recreate the assistant when the model changes after going back", async () => {
        const user = userEvent.setup();
        const add = vi.fn().mockReturnValueOnce(detail(9, 0)).mockReturnValueOnce(detail(10, 0));
        mockInvokeHandler("add_assistant", add);
        const remove = vi.fn();
        mockInvokeHandler("delete_assistant", remove);
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await fillBasics(user, { model: "Test model" });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("region", { name: "高级配置" });

        await user.click(screen.getByRole("button", { name: "上一步" }));
        await user.click(screen.getByRole("button", { name: "上一步" }));
        await user.click(screen.getAllByRole("combobox")[1]);
        await user.click(screen.getByRole("option", { name: "Other model" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));

        await waitFor(() => expect(add).toHaveBeenCalledTimes(2));
        expect(remove).toHaveBeenCalledWith({ assistantId: 9 });
        expect(add.mock.calls[1][0]).toMatchObject({ modelSelection: "other%%10" });
    });

    it("should create a normal assistant with the prompt written in the first step", async () => {
        const user = userEvent.setup();
        const add = vi.fn().mockReturnValue(detail(9, 0));
        mockInvokeHandler("add_assistant", add);
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await user.click(screen.getByRole("button", { name: "add" }));
        fireEvent.change(screen.getByLabelText("提示词（选填）"), { target: { value: "你是写作助手" } });
        await user.click(screen.getByRole("button", { name: "取消" }));
        // 重新打开从空白开始
        await fillBasics(user, { model: "Test model" });
        await user.click(screen.getByRole("button", { name: "上一步" }));
        fireEvent.change(screen.getByLabelText("提示词（选填）"), { target: { value: "你是写作助手" } });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await waitFor(() => expect(add).toHaveBeenCalledWith(expect.objectContaining({ prompt: "你是写作助手" })));
    });

    it("should save a prompt edited after the assistant was created", async () => {
        const user = userEvent.setup();
        const add = vi.fn().mockReturnValue(detail(9, 0));
        mockInvokeHandler("add_assistant", add);
        const save = vi.fn();
        mockInvokeHandler("save_assistant", save);
        mockInvokeHandler("get_assistant", () => detail(9, 0));
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await fillBasics(user, { model: "Test model" });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("region", { name: "高级配置" });

        await user.click(screen.getByRole("button", { name: "上一步" }));
        await user.click(screen.getByRole("button", { name: "上一步" }));
        fireEvent.change(screen.getByLabelText("提示词（选填）"), { target: { value: "新的提示词" } });
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("region", { name: "高级配置" });
        await user.click(screen.getByRole("button", { name: "完成" }));

        await waitFor(() => expect(save).toHaveBeenCalledOnce());
        // 只改提示词不重建助手
        expect(add).toHaveBeenCalledOnce();
        expect((save.mock.calls[0][0] as { assistantDetail: AssistantDetail }).assistantDetail.prompts[0]).toMatchObject({ id: 1, prompt: "新的提示词" });
    });

    it("should show agent settings instead of model parameters for agent assistants", async () => {
        const user = userEvent.setup();
        const add = vi.fn().mockReturnValue(detail(11, 4));
        mockInvokeHandler("add_assistant", add);
        mockInvokeHandler("get_agent_runtime_info", () => ({ agent_kind: "acp" }));
        render(<AddAssistantDialog assistantTypes={types} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await user.click(screen.getByRole("button", { name: "add" }));
        await user.click(screen.getAllByRole("combobox")[0]);
        await user.click(screen.getByRole("option", { name: "Agent" }));
        // 智能体助手没有提示词输入
        expect(screen.queryByLabelText("提示词（选填）")).not.toBeInTheDocument();
        fireEvent.change(screen.getByLabelText("名称"), { target: { value: "New" } });
        await user.click(screen.getAllByRole("combobox")[1]);
        await user.click(screen.getByRole("option", { name: "Agent provider" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));
        await user.click(screen.getByRole("button", { name: "下一步" }));
        const advanced = await screen.findByRole("region", { name: "高级配置" });
        expect(within(advanced).getByText("环境变量")).toBeInTheDocument();
        expect(within(advanced).getByText("附加启动参数")).toBeInTheDocument();
        expect(within(advanced).queryByText("温度")).not.toBeInTheDocument();
        // 工作目录、提供商属于基本配置，不在这一步
        expect(within(advanced).queryByText("工作目录")).not.toBeInTheDocument();
        expect(screen.queryByRole("region", { name: "扩展配置" })).not.toBeInTheDocument();
        expect(add.mock.calls[0][0]).toMatchObject({ assistantType: 4, prompt: undefined });
    });
});
