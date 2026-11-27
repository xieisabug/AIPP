import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import type { AssistantDetail } from "@/data/Assistant";
import type { LoadedPlugin } from "@/services/PluginRuntime";
import AssistantConfig from "./AssistantConfig";

// Exercise the actual editor, field registration, change handlers, footer and save flow.
// These independent dialogs and panels manage their own data.
vi.mock("./AddAssistantDialog", () => ({ default: () => null }));
vi.mock("./assistant/AssistantDialogs", () => ({ AssistantDialogs: () => null }));
vi.mock("./AssistantMCPFieldDisplay", () => ({ default: () => null }));
vi.mock("./AssistantSkillsFieldDisplay", () => ({ default: () => null }));
vi.mock("./AssistantWorkspaceFieldDisplay", () => ({ default: () => null }));
vi.mock("@/hooks/feature/useFeatureConfig", () => ({
    useFeatureConfig: () => ({ featureConfig: new Map(), getConfigValue: () => "" }),
}));
vi.mock("@/hooks/useFilteredModels", () => ({
    useFilteredModels: () => ({
        models: ["Alpha", "Beta"].map(code => ({ code, name: code, llm_provider_id: 3 })),
        loading: false, error: null,
    }),
}));

function assistant(id: number): AssistantDetail {
    return {
        assistant: { id, name: `助手 ${id}`, assistant_type: 0, description: null, is_addition: false, created_time: "" },
        prompts: [{ id, assistant_id: id, prompt: `提示词 ${id}`, created_time: "" }],
        model: [{ id, assistant_id: id, model_code: "Alpha", provider_id: 3, alias: "" }],
        model_configs: [], prompt_params: [], mcp_configs: [], mcp_tool_configs: [],
    };
}

async function openEditor(first = assistant(1), plugins: LoadedPlugin[] = []) {
    vi.stubGlobal("innerWidth", 1440);
    const records = new Map([[1, first], [2, assistant(2)]]);
    const save = vi.fn(async (detail: AssistantDetail) => { records.set(detail.assistant.id, detail); });
    mockInvokeHandler("get_assistants", () => Array.from(records.values(), detail => detail.assistant));
    mockInvokeHandler("get_assistant", args => structuredClone(records.get(Number(args?.assistantId))));
    mockInvokeHandler("save_assistant", args => save(args?.assistantDetail as AssistantDetail));
    const user = userEvent.setup();
    render(<AssistantConfig pluginList={plugins} navigateTo={() => undefined} />);
    const prompt = await screen.findByDisplayValue("提示词 1");
    const button = screen.getByRole("button", { name: "保存配置" });
    expect(button).toBeDisabled();
    return { prompt, button, save, user };
}

async function chooseOption(trigger: HTMLElement, name: string) {
    fireEvent.keyDown(trigger, { key: "Enter" });
    fireEvent.keyDown(await screen.findByRole("option", { name }), { key: "Enter" });
}

describe("assistant configuration save button", () => {
    afterEach(() => { clearAllMockHandlers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

    it("should stay disabled when browsing tabs or focusing an unchanged field", async () => {
        const { prompt, button, user, save } = await openEditor();
        await user.click(prompt);
        await user.click(screen.getByRole("tab", { name: "参数与高级" }));
        await user.click(screen.getByRole("tab", { name: "角色与模型" }));
        expect(button).toBeDisabled();
        await user.click(button);
        expect(save).not.toHaveBeenCalled();
    });

    it("should track a model selection and persist the selected model", async () => {
        const { button, save } = await openEditor();
        const role = screen.getByRole("tabpanel", { name: "角色与模型" });
        const model = within(role).getByText("Alpha").closest("button")!;
        await chooseOption(model, "Beta");
        await waitFor(() => expect(button).toBeEnabled());
        await chooseOption(model, "Alpha");
        await waitFor(() => expect(button).toBeDisabled());
        await chooseOption(model, "Beta");
        await waitFor(() => expect(button).toBeEnabled());
        fireEvent.click(button);
        await waitFor(() => expect(save).toHaveBeenCalledOnce());
        expect(save.mock.calls[0][0].model[0]).toMatchObject({ model_code: "Beta", provider_id: 3 });
        expect(save.mock.calls[0][0].model_configs.map(config => config.name)).not.toContain("use_native_toolcall");
        await waitFor(() => expect(button).toBeDisabled());
    });

    it("should stay enabled until both the prompt and a default-off tool switch are restored", async () => {
        const { prompt, button, user } = await openEditor();
        await user.type(prompt, "!");
        expect(button).toBeEnabled();
        await user.click(screen.getByRole("tab", { name: "能力与工具" }));
        const toggle = within(screen.getByRole("tabpanel", { name: "能力与工具" })).getByRole("switch");
        expect(toggle).not.toBeChecked();
        await user.click(toggle);
        await user.click(screen.getByRole("tab", { name: "角色与模型" }));
        fireEvent.change(prompt, { target: { value: "提示词 1" } });
        await user.click(screen.getByRole("tab", { name: "能力与工具" }));
        expect(button).toBeEnabled();
        await user.click(toggle);
        expect(toggle).not.toBeChecked();
        expect(button).toBeDisabled();
    });

    it("should discard edits and track the selected assistant's own saved prompt", async () => {
        const { prompt, button, user, save } = await openEditor();
        await user.clear(prompt);
        expect(button).toBeEnabled();
        await user.click(screen.getByRole("button", { name: "放弃更改" }));
        expect(prompt).toHaveValue("提示词 1");
        expect(button).toBeDisabled();
        await user.click(screen.getByRole("button", { name: "助手 2" }));
        const next = await screen.findByDisplayValue("提示词 2");
        const nextSave = screen.getByRole("button", { name: "保存配置" });
        expect(nextSave).toBeDisabled();
        fireEvent.change(next, { target: { value: "提示词 1" } });
        await waitFor(() => expect(nextSave).toBeEnabled());
        expect(save).not.toHaveBeenCalled();
    });

    it("should track a plugin field independently and save its edited value for the current assistant", async () => {
        const plugin = {
            pluginId: 8, code: "notes", name: "Notes", pluginType: [],
            contributions: { assistantFormFields: [{ key: "note", type: "text", label: "备注" }] },
        } as unknown as LoadedPlugin;
        let persisted = "原备注";
        mockInvokeHandler("get_plugin_assistant_configs", () => [{ configKey: "note", configValue: persisted }]);
        const write = vi.fn(args => { persisted = args.value; });
        mockInvokeHandler("set_plugin_assistant_config", write);
        const { user, button } = await openEditor(assistant(1), [plugin]);
        await user.click(screen.getByRole("tab", { name: "参数与高级" }));
        const note = screen.getByDisplayValue("原备注");
        await user.type(note, "!");
        expect(button).toBeEnabled();
        await user.keyboard("{Backspace}");
        expect(button).toBeDisabled();
        await user.type(note, "?");
        await user.click(button);
        expect(write).toHaveBeenCalledWith({ assistantId: 1, pluginId: 8, key: "note", value: "原备注?" });
        await waitFor(() => expect(button).toBeDisabled());
    });

    it("should persist a legacy agent's first opt-out from dynamic MCP loading", async () => {
        const agent = assistant(1);
        agent.assistant.assistant_type = 4;
        const save = vi.fn(async (_detail: AssistantDetail) => {});
        mockInvokeHandler("get_assistants", () => [agent.assistant]);
        mockInvokeHandler("get_assistant", () => agent);
        mockInvokeHandler("get_agent_runtime_info", () => ({ agent_kind: "antigravity" }));
        mockInvokeHandler("get_filtered_providers", () => [{ id: 3, name: "Antigravity", api_type: "antigravity" }]);
        mockInvokeHandler("get_agent_model_options", () => []);
        mockInvokeHandler("get_llm_provider_config", () => []);
        mockInvokeHandler("save_assistant", args => save(args?.assistantDetail as AssistantDetail));
        const user = userEvent.setup();
        render(<AssistantConfig pluginList={[]} navigateTo={() => undefined} />);
        const button = await screen.findByRole("button", { name: "保存配置" });
        await user.click(screen.getByRole("tab", { name: "能力与工具" }));
        const panel = screen.getByRole("tabpanel", { name: "能力与工具" });
        await user.click(within(panel).getByRole("switch", { checked: true }));
        await user.click(button);
        await waitFor(() => expect(save).toHaveBeenCalledOnce());
        expect(save.mock.calls[0][0].model_configs).toContainEqual(expect.objectContaining({
            name: "dynamic_mcp_loading_enabled", value_type: "boolean", value: "false",
        }));
    });

    it("should restore both tool switches to their displayed defaults when edits are discarded", async () => {
        const first = assistant(1);
        first.model_configs.push({
            id: 1, assistant_id: 1, assistant_model_id: 1,
            name: "mcp_loading_mode_explicit", value_type: "boolean", value: "true",
        });
        const { user, button } = await openEditor(first);
        await user.click(screen.getByRole("tab", { name: "能力与工具" }));
        const panel = screen.getByRole("tabpanel", { name: "能力与工具" });
        const dynamic = within(panel).getByRole("switch", { checked: true });
        const native = within(panel).getByRole("switch", { checked: false });
        await user.click(dynamic);
        expect(button).toBeEnabled();
        await user.click(dynamic);
        expect(button).toBeDisabled();
        await user.click(native);
        await user.click(dynamic);
        await user.click(screen.getByRole("button", { name: "放弃更改" }));
        expect(native).not.toBeChecked();
        expect(dynamic).toBeChecked();
        expect(button).toBeDisabled();
    });

    it("should enable retry after a failed save and disable after a successful retry", async () => {
        const { prompt, button, user, save } = await openEditor();
        save.mockRejectedValueOnce(new Error("保存失败"));
        await user.type(prompt, "!");
        await user.click(button);
        expect(save).toHaveBeenCalledTimes(1);
        expect(prompt).toHaveValue("提示词 1!");
        expect(button).toBeEnabled();
        await user.click(button);
        await waitFor(() => expect(button).toBeDisabled());
        expect(save).toHaveBeenCalledTimes(2);
        expect(save.mock.calls[1][0].prompts[0].prompt).toBe("提示词 1!");
    });

    it.each([
        ["沙箱模式", "工作区可写", "只读"],
        ["审批策略", "按需询问", "仅读命令自动执行"],
        ["审批人", "人工审批", "自动审批"],
    ])("should stay clean after loading Agent defaults and restoring inherited %s", async (_field, providerValue, override) => {
        const agent = assistant(1);
        agent.assistant.assistant_type = 4;
        agent.model[0].model_code = "";
        mockInvokeHandler("get_assistants", () => [agent.assistant]);
        mockInvokeHandler("get_assistant", () => agent);
        mockInvokeHandler("get_agent_runtime_info", () => ({ agent_kind: "codex_app_server" }));
        mockInvokeHandler("get_filtered_providers", () => [{ id: 3, name: "Codex", api_type: "codex_app_server" }]);
        mockInvokeHandler("get_llm_provider_config", () => [{ name: "codex_sandbox", value: "workspace-write" }]);
        mockInvokeHandler("get_agent_model_options", () => [{
            code: "Alpha%%3", name: "Alpha", provider_id: 3, is_default: true,
            efforts: ["medium", "high"], default_effort: "medium",
        }]);
        render(<AssistantConfig pluginList={[]} navigateTo={() => undefined} />);
        await screen.findByText("Alpha");
        const button = screen.getByRole("button", { name: "保存配置" });
        expect(button).toBeDisabled();
        await userEvent.setup().click(screen.getByRole("tab", { name: "权限与工作区" }));
        const inherited = `跟随提供商（当前：${providerValue}）`;
        const select = screen.getByText(inherited).closest("button")!;
        await chooseOption(select, override);
        await waitFor(() => expect(button).toBeEnabled());
        await chooseOption(select, inherited);
        await waitFor(() => expect(button).toBeDisabled());
        expect(select).toHaveTextContent(inherited);
    });

    it("should preserve edits made during a pending save and compare against the submitted prompt", async () => {
        const { prompt, button, user, save } = await openEditor();
        let finish!: () => void;
        save.mockImplementationOnce(() => new Promise<void>(resolve => { finish = resolve; }));
        await user.type(prompt, "!");
        await user.click(button);
        expect(screen.getByRole("button", { name: "保存中…" })).toBeDisabled();
        await user.type(prompt, "?");
        await user.click(button);
        expect(save).toHaveBeenCalledOnce();
        await act(async () => finish());
        expect(prompt).toHaveValue("提示词 1!?");
        expect(button).toBeEnabled();
        fireEvent.change(prompt, { target: { value: "提示词 1!" } });
        await waitFor(() => expect(button).toBeDisabled());
    });
});
