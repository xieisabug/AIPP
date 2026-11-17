import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "../../__tests__/mocks/tauri";
import NewProviderWizard from "./NewProviderWizard";
import { PROVIDER_API_TYPES, newProviderNameError, suggestProviderName } from "./providerApiTypes";

const DRAFT_ID = 42;

function model(code: string, extra: Record<string, unknown> = {}) {
    return {
        code, name: code, description: "", vision_support: false, audio_support: false, video_support: false,
        request_mode: "chat_completions", is_selected: false, ...extra,
    };
}

interface SetupOptions {
    existing?: Array<{ id: string; name: string; api_type?: string }>;
    previewError?: string;
    /** check_acp_library：已安装的命令 → 版本 */
    installedClis?: Record<string, string>;
    /** detect_native_agent_environment：已安装的原生智能体 */
    installedNative?: string[];
    defaults?: Record<string, unknown>;
}

function nativeEnvironment(driver: string, installed: boolean) {
    const program = { path: `/bin/${driver}`, source: "PATH", version: "1.2.3", ok: installed, warning: null, error: installed ? null : "missing" };
    return {
        driver,
        node: { configured: "", required: "22.19.0", selected: { ...program, path: "/bin/node", version: "22.20.0", ok: true }, candidates: [] },
        tool: { kind: "cli", display_name: `${driver} CLI`, default_command: driver, configured: "", selected: program, candidates: [],
            install_commands: [], docs_url: "", can_update: false, managed_directory: null, warnings: [], account: null, uses_api_key: false },
        working_directory: { configured: "", resolved: "", exists: true },
    };
}

function setup(options: SetupOptions = {}) {
    mockInvokeHandler("get_platform", () => "windows");
    mockInvokeHandler("check_acp_library", (args) => {
        const command = String(args?.cliCommand);
        const version = options.installedClis?.[command];
        return { cli_command: command, package_name: command, installed: Boolean(version), version: version ?? null,
            installed_path: version ? `/usr/bin/${command}` : null, requires_external_install: false, install_hint: "" };
    });
    mockInvokeHandler("detect_native_agent_environment", (args) =>
        nativeEnvironment(String(args?.driver), options.installedNative?.includes(String(args?.driver)) ?? false));
    mockInvokeHandler("read_local_agent_defaults", (args) =>
        options.defaults?.[String(args?.apiType)] ?? { source: null, found: false, settings: {}, info: [], notes: [] });
    mockInvokeHandler("vault_capability", () => ({ available: false }));
    mockInvokeHandler("add_llm_provider", () => DRAFT_ID);
    mockInvokeHandler("update_llm_provider", () => undefined);
    mockInvokeHandler("update_llm_provider_config", () => undefined);
    mockInvokeHandler("delete_llm_provider", () => undefined);
    mockInvokeHandler("update_selected_models", () => undefined);
    mockInvokeHandler("add_llm_model", () => ({}));
    mockInvokeHandler("preview_model_list", () => {
        if (options.previewError) throw options.previewError;
        return { available_models: [model("gpt-a"), model("gpt-b")], missing_models: [] };
    });
    const onClose = vi.fn();
    const onCreated = vi.fn();
    const utils = render(
        <NewProviderWizard
            open
            onClose={onClose}
            apiTypes={PROVIDER_API_TYPES}
            existingProviders={options.existing ?? [{ id: "1", name: "OpenAI API" }]}
            onCreated={onCreated}
        />,
    );
    return { ...utils, onClose, onCreated };
}

const configWrites = () => invoke.mock.calls
    .filter(([command]) => command === "update_llm_provider_config")
    .map(([, args]) => [(args as { name: string }).name, (args as { value: string }).value]);

describe("NewProviderWizard", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
    });

    it("should list API types as a grid of buttons instead of a dropdown", () => {
        setup();
        expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
        expect(screen.getByTestId("wizard-step")).toHaveTextContent("1/3 · 选择 API 类型");
        for (const label of ["OpenAI API", "Anthropic API", "Codex", "GitHub Copilot"]) {
            expect(screen.getByRole("button", { name: new RegExp(label) })).toBeInTheDocument();
        }
    });

    it("should prefill a unique name and the default endpoint when a type is chosen", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /OpenAI API/ }));
        expect(await screen.findByLabelText("提供商名称")).toHaveValue("OpenAI API 2");
        expect(screen.getByLabelText("接口地址")).toHaveValue("https://api.openai.com/v1/");
        expect(screen.getByTestId("wizard-step")).toHaveTextContent("2/3 · 填写连接信息");
    });

    it("should not create a provider when the name is empty or duplicated", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /OpenAI API/ }));
        const name = await screen.findByLabelText("提供商名称");
        fireEvent.change(name, { target: { value: "  " } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("请填写提供商名称");
        fireEvent.change(name, { target: { value: " OpenAI API " } });
        fireEvent.keyDown(name, { key: "Enter" });
        expect(await screen.findByRole("alert")).toHaveTextContent("已有同名提供商");
        expect(invoke).not.toHaveBeenCalledWith("add_llm_provider", expect.anything());
    });

    it("should not submit while the IME is composing", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.keyDown(await screen.findByLabelText("提供商名称"), { key: "Enter", isComposing: true });
        expect(invoke).not.toHaveBeenCalledWith("add_llm_provider", expect.anything());
    });

    it("should block an invalid endpoint", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.change(await screen.findByLabelText("接口地址"), { target: { value: "bad" } });
        expect(screen.getByRole("alert")).toHaveTextContent("接口地址无效");
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        expect(invoke).not.toHaveBeenCalledWith("add_llm_provider", expect.anything());
    });

    it("should save the connection, fetch models and enable the provider on finish", async () => {
        const { onCreated, onClose } = setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.change(await screen.findByLabelText("提供商名称"), { target: { value: "  我的 DeepSeek  " } });
        fireEvent.change(screen.getByLabelText("API Key"), { target: { value: " sk-test " } });
        fireEvent.click(screen.getByRole("button", { name: /高级配置/ }));
        fireEvent.click(await screen.findByRole("switch"));
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));

        await screen.findByRole("checkbox", { name: "gpt-a" });
        expect(invoke).toHaveBeenCalledWith("add_llm_provider", { name: "我的 DeepSeek", apiType: "deepseek" });
        // 接口地址先于 API Key 写入（保险箱引用按当前地址确认）
        expect(configWrites()).toEqual([
            ["endpoint", "https://api.deepseek.com/"],
            ["proxy_enabled", "true"],
            ["api_key", "sk-test"],
        ]);
        expect(invoke).toHaveBeenCalledWith("preview_model_list", { llmProviderId: DRAFT_ID });
        expect(screen.getByTestId("wizard-step")).toHaveTextContent("3/3 · 选择模型");

        fireEvent.click(screen.getByRole("checkbox", { name: "gpt-b" }));
        fireEvent.change(screen.getByLabelText("自定义模型"), { target: { value: "my-model" } });
        fireEvent.keyDown(screen.getByLabelText("自定义模型"), { key: "Enter" });
        expect(screen.getByText("已选 2 个模型")).toBeInTheDocument();

        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(onCreated).toHaveBeenCalledWith(DRAFT_ID));
        const selection = invoke.mock.calls.find(([command]) => command === "update_selected_models")?.[1] as {
            selectedModels: Array<{ code: string; is_selected: boolean }>;
        };
        expect(selection.selectedModels.filter((m) => m.is_selected).map((m) => m.code)).toEqual(["gpt-b"]);
        expect(invoke).toHaveBeenCalledWith("add_llm_model", { code: "my-model", llmProviderId: DRAFT_ID });
        expect(invoke).toHaveBeenLastCalledWith("update_llm_provider", {
            id: DRAFT_ID, name: "我的 DeepSeek", apiType: "deepseek", description: "", isEnabled: true,
        });
        expect(onClose).toHaveBeenCalled();
    });

    it("should select a fetched model instead of duplicating it as a custom model", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        await screen.findByRole("checkbox", { name: "gpt-a" });
        fireEvent.change(screen.getByLabelText("自定义模型"), { target: { value: "gpt-a" } });
        fireEvent.click(screen.getByRole("button", { name: "添加" }));
        expect(screen.getByRole("checkbox", { name: "gpt-a" })).toBeChecked();
        expect(screen.getByText("已选 1 个模型")).toBeInTheDocument();
    });

    it("should still allow custom models when fetching fails", async () => {
        const { onCreated } = setup({ previewError: "401 Unauthorized" });
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        const alert = await screen.findByRole("alert");
        expect(within(alert).getByText("401 Unauthorized")).toBeInTheDocument();
        fireEvent.change(screen.getByLabelText("自定义模型"), { target: { value: "deepseek-chat" } });
        fireEvent.click(screen.getByRole("button", { name: "添加" }));
        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(onCreated).toHaveBeenCalledWith(DRAFT_ID));
        expect(invoke).not.toHaveBeenCalledWith("update_selected_models", expect.anything());
        expect(invoke).toHaveBeenCalledWith("add_llm_model", { code: "deepseek-chat", llmProviderId: DRAFT_ID });
    });

    it("should update the existing draft instead of creating another when going back", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        await screen.findByRole("checkbox", { name: "gpt-a" });
        fireEvent.click(screen.getByRole("button", { name: /上一步/ }));
        fireEvent.change(await screen.findByLabelText("提供商名称"), { target: { value: "改名" } });
        fireEvent.click(screen.getByRole("button", { name: "下一步" }));
        await screen.findByRole("checkbox", { name: "gpt-a" });
        expect(invoke.mock.calls.filter(([command]) => command === "add_llm_provider")).toHaveLength(1);
        expect(invoke).toHaveBeenCalledWith("update_llm_provider", {
            id: DRAFT_ID, name: "改名", apiType: "deepseek", description: "", isEnabled: false,
        });
    });

    it("should delete the old draft when switching to another type", async () => {
        setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        await screen.findByRole("checkbox", { name: "gpt-a" });
        fireEvent.click(screen.getByRole("button", { name: /上一步/ }));
        fireEvent.click(await screen.findByRole("button", { name: "更换" }));
        fireEvent.click(await screen.findByRole("button", { name: /Anthropic API/ }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("delete_llm_provider", { llmProviderId: DRAFT_ID }));
        expect(await screen.findByLabelText("提供商名称")).toHaveValue("Anthropic API");
        expect(screen.getByLabelText("接口地址")).toHaveValue("https://api.anthropic.com/");
    });

    it("should delete the unfinished draft when the wizard is closed", async () => {
        const { rerender, onClose, onCreated } = setup();
        fireEvent.click(screen.getByRole("button", { name: /DeepSeek/ }));
        fireEvent.click(await screen.findByRole("button", { name: "下一步" }));
        await screen.findByRole("checkbox", { name: "gpt-a" });
        rerender(
            <NewProviderWizard open={false} onClose={onClose} apiTypes={PROVIDER_API_TYPES} existingProviders={[]} onCreated={onCreated} />,
        );
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("delete_llm_provider", { llmProviderId: DRAFT_ID }));
        expect(onCreated).not.toHaveBeenCalled();
    });

    it("should finish at step two for local agents without fetching models", async () => {
        const { onCreated } = setup();
        fireEvent.click(screen.getByRole("button", { name: /Codex/ }));
        expect(await screen.findByTestId("wizard-step")).toHaveTextContent("2/2 · 确认本机配置");
        expect(screen.queryByLabelText("接口地址")).not.toBeInTheDocument();
        expect(screen.queryByLabelText("API Key")).not.toBeInTheDocument();
        await act(async () => {
            fireEvent.click(screen.getByRole("button", { name: "完成" }));
        });
        await waitFor(() => expect(onCreated).toHaveBeenCalledWith(DRAFT_ID));
        expect(invoke).not.toHaveBeenCalledWith("preview_model_list", expect.anything());
        // 没有 ~/.codex/config.toml 时写入与详情页一致的默认值
        expect(configWrites()).toEqual([["codex_approval_policy", "on-request"], ["codex_sandbox", "workspace-write"]]);
        expect(invoke).toHaveBeenCalledWith("update_llm_provider", {
            id: DRAFT_ID, name: "Codex", apiType: "codex_app_server", description: "", isEnabled: true,
        });
    });

    it("should mark installed local agents and types that already have providers", async () => {
        setup({
            existing: [{ id: "1", name: "OpenAI API", api_type: "openai_api" }, { id: "2", name: "Work", api_type: "openai_api" }, { id: "3", name: "Pi", api_type: "pi" }],
            installedClis: { codex: "0.61.0" },
            installedNative: ["kiro"],
        });
        const card = (name: RegExp) => screen.getByRole("button", { name });
        await waitFor(() => expect(within(card(/Codex/)).getByText("本机已安装")).toBeInTheDocument());
        await waitFor(() => expect(within(card(/Kiro/)).getByText("本机已安装")).toBeInTheDocument());
        await waitFor(() => expect(screen.queryByText("检测本机安装…")).not.toBeInTheDocument());
        expect(within(card(/Claude Code/)).queryByText("本机已安装")).not.toBeInTheDocument();
        expect(within(card(/OpenAI API/)).getByText("已添加 2 个")).toBeInTheDocument();
        expect(within(card(/^Pi/)).getByText("已添加")).toBeInTheDocument();
        expect(within(card(/^Pi/)).queryByText("本机已安装")).not.toBeInTheDocument();
    });

    it("should prefill Codex settings from its own config and save them", async () => {
        const { onCreated } = setup({
            installedClis: { codex: "0.61.0" },
            defaults: {
                codex_app_server: {
                    source: "~/.codex/config.toml", found: true,
                    settings: { codex_approval_policy: "never", codex_sandbox: "read-only" },
                    info: [{ label: "默认模型", value: "gpt-5-codex" }],
                    notes: [],
                },
            },
        });
        fireEvent.click(screen.getByRole("button", { name: /Codex/ }));
        const defaults = await screen.findByTestId("local-agent-defaults");
        await waitFor(() => expect(within(defaults).getByText("gpt-5-codex")).toBeInTheDocument());
        expect(screen.getByText("~/.codex/config.toml")).toBeInTheDocument();
        expect(within(defaults).getByText("从不询问")).toBeInTheDocument();
        expect(within(defaults).getByText("只读")).toBeInTheDocument();
        await waitFor(() => expect(screen.getByText(/Codex CLI 0\.61\.0/)).toBeInTheDocument());
        expect(screen.getByText("/usr/bin/codex")).toBeInTheDocument();
        expect(invoke).toHaveBeenCalledWith("read_local_agent_defaults", { apiType: "codex_app_server" });

        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(onCreated).toHaveBeenCalledWith(DRAFT_ID));
        expect(configWrites()).toEqual([["codex_approval_policy", "never"], ["codex_sandbox", "read-only"]]);
    });

    it("should show Claude Code defaults without pinning them on the provider", async () => {
        const { onCreated } = setup({
            defaults: {
                claude_sdk: {
                    source: "~/.claude/settings.json", found: true, settings: {},
                    info: [{ label: "权限模式", value: "acceptEdits" }, { label: "思考强度", value: "xhigh" }],
                    notes: [],
                },
            },
        });
        fireEvent.click(screen.getByRole("button", { name: /Claude Code/ }));
        const defaults = await screen.findByTestId("local-agent-defaults");
        await waitFor(() => expect(within(defaults).getByText("acceptEdits")).toBeInTheDocument());
        expect(within(defaults).getByText("xhigh")).toBeInTheDocument();
        expect(screen.getByText(/AIPP 不覆盖这些设置/)).toBeInTheDocument();
        // 未安装 Claude CLI 时如实提示
        await waitFor(() => expect(screen.getByText(/未检测到 Claude Code CLI/)).toBeInTheDocument());

        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(onCreated).toHaveBeenCalledWith(DRAFT_ID));
        expect(configWrites()).toEqual([]);
    });

    it("should show read notes when a config value cannot be used", async () => {
        setup({
            defaults: {
                codex_app_server: {
                    source: "~/.codex/config.toml", found: true, settings: {}, info: [],
                    notes: ["approval_policy = \"on-failure\" 不在 AIPP 支持的取值内（untrusted / on-request / never），将使用 on-request"],
                },
            },
        });
        fireEvent.click(screen.getByRole("button", { name: /Codex/ }));
        expect(await screen.findByText(/on-failure/)).toBeInTheDocument();
        expect(within(screen.getByTestId("local-agent-defaults")).getByText("按需询问")).toBeInTheDocument();
    });

    it("should select the first installed ACP CLI and save it", async () => {
        const { onCreated } = setup({ installedClis: { kimi: "1.0.0", gemini: "0.9.0" } });
        fireEvent.click(screen.getByRole("button", { name: /ACP/ }));
        // 按选项顺序取第一个已安装的：gemini 在 kimi 之前
        await waitFor(() => expect(screen.getByLabelText("CLI 命令")).toHaveTextContent(/Gemini CLI/));
        fireEvent.click(screen.getByRole("button", { name: "完成" }));
        await waitFor(() => expect(onCreated).toHaveBeenCalledWith(DRAFT_ID));
        expect(configWrites()).toEqual([["proxy_enabled", "false"], ["acp_cli_command", "gemini"]]);
    });

    it("should show the detected program and Node.js for native agents", async () => {
        setup({
            installedNative: ["opencode"],
            defaults: { opencode: { source: "~/.config/opencode/opencode.json", found: false, settings: {}, info: [], notes: [] } },
        });
        fireEvent.click(screen.getByRole("button", { name: /^OpenCode\s*本机/ }));
        await waitFor(() => expect(screen.getByText(/opencode CLI 1\.2\.3/)).toBeInTheDocument());
        expect(screen.getByText("v22.20.0")).toBeInTheDocument();
        expect(await screen.findByText(/没有自定义配置，OpenCode 将使用它自己的默认值/)).toBeInTheDocument();
    });
});

describe("providerApiTypes helpers", () => {
    it("should suggest the next free numbered name", () => {
        expect(suggestProviderName("OpenAI API", [])).toBe("OpenAI API");
        expect(suggestProviderName("OpenAI API", ["OpenAI API", "OpenAI API 2"])).toBe("OpenAI API 3");
    });

    it("should reject empty and duplicate names after trimming", () => {
        expect(newProviderNameError("  ", [])).toBe("请填写提供商名称");
        expect(newProviderNameError(" A ", ["A"])).toBe("已有同名提供商");
        expect(newProviderNameError("B", ["A"])).toBeNull();
    });
});
