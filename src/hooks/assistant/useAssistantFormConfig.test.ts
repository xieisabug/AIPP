import React from "react";
import { render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { emit } from "@tauri-apps/api/event";

import type { AssistantDetail } from "@/data/Assistant";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { useAssistantFormConfig } from "./useAssistantFormConfig";

const baseAssistantDetail: AssistantDetail = {
    assistant: {
        id: 1,
        name: "Test Assistant",
        description: null,
        assistant_type: 0,
        is_addition: false,
        created_time: "2026-01-01T00:00:00Z",
    },
    prompts: [
        {
            id: 1,
            assistant_id: 1,
            prompt: "test prompt",
            created_time: "2026-01-01T00:00:00Z",
        },
    ],
    model: [
        {
            id: 1,
            assistant_id: 1,
            model_code: "gpt-test",
            provider_id: 1,
            alias: "",
        },
    ],
    model_configs: [],
    prompt_params: [],
    mcp_configs: [],
    mcp_tool_configs: [],
};

const pluginAssistantFormFields = [
    {
        pluginId: 9,
        pluginCode: "hidden-first-turn-context-plugin",
        pluginName: "Hidden First Turn Context Plugin",
        formKey: "plugin::9::enabled",
        key: "enabled",
        label: "启用首轮隐藏上下文",
        type: "checkbox",
        description: "打开后，仅在该助手的第一轮模型请求中注入隐藏上下文。",
        defaultValue: false,
        options: [],
    },
    {
        pluginId: 9,
        pluginCode: "hidden-first-turn-context-plugin",
        pluginName: "Hidden First Turn Context Plugin",
        formKey: "plugin::9::hiddenContext",
        key: "hiddenContext",
        label: "首轮隐藏上下文",
        type: "textarea",
        description: "不会直接显示在聊天 UI 中，只会在首轮请求发给模型时附加。",
        defaultValue: "",
        options: [],
    },
    {
        pluginId: 9,
        pluginCode: "hidden-first-turn-context-plugin",
        pluginName: "Hidden First Turn Context Plugin",
        formKey: "plugin::9::injectionRole",
        key: "injectionRole",
        label: "注入角色",
        type: "select",
        description: "决定隐藏上下文以 system 还是 user 消息注入到模型请求。",
        defaultValue: "system",
        options: [
            { value: "system", label: "System" },
            { value: "user", label: "User" },
        ],
    },
];

describe("useAssistantFormConfig plugin field visibility", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_all_feature_config", () => []);
    });

    it("hides dependent plugin fields until the enabled checkbox is checked", async () => {
        const onConfigChange = vi.fn();
        const onPromptChange = vi.fn();
        const onPluginConfigChange = vi.fn();

        const { result, rerender } = renderHook(
            ({ pluginAssistantConfigValues }: { pluginAssistantConfigValues: Record<string, string | boolean> }) =>
                useAssistantFormConfig({
                    currentAssistant: baseAssistantDetail,
                    assistantTypeNameMap: new Map([[0, "普通对话助手"]]),
                    assistantTypeCustomField: [],
                    assistantTypeCustomLabel: new Map(),
                    assistantTypeCustomTips: new Map(),
                    assistantTypeHideField: [],
                    navigateTo: vi.fn(),
                    onConfigChange,
                    onPromptChange,
                    pluginAssistantFormFields,
                    pluginAssistantConfigValues,
                    onPluginConfigChange,
                    agentModelOptions: [],
                    agentModelLoading: false,
                    agentModelError: null,
                    onAgentProviderChange: vi.fn(),
                    onAgentModelChange: vi.fn(),
                }),
            {
                initialProps: {
                    pluginAssistantConfigValues: {
                        "plugin::9::enabled": false,
                    },
                },
            }
        );

        await waitFor(() => {
            expect(result.current.formConfig.length).toBeGreaterThan(0);
        });

        expect(
            result.current.formConfig.some((item) => item.key === "plugin-group::9")
        ).toBe(false);
        expect(
            result.current.formConfig.find((item) => item.key === "plugin::9::hiddenContext")?.config.hidden
        ).toBe(true);
        expect(
            result.current.formConfig.find((item) => item.key === "plugin::9::injectionRole")?.config.hidden
        ).toBe(true);

        rerender({
            pluginAssistantConfigValues: {
                "plugin::9::enabled": true,
            },
        });

        expect(
            result.current.formConfig.find((item) => item.key === "plugin::9::hiddenContext")?.config.hidden
        ).toBe(false);
        expect(
            result.current.formConfig.find((item) => item.key === "plugin::9::injectionRole")?.config.hidden
        ).toBe(false);
    });
});

it("should offer the shared default prompt only when editing the quick assistant", async () => {
    const onPromptChange = vi.fn();
    const { result, rerender } = renderHook(({ id }) => useAssistantFormConfig({
        currentAssistant: { ...baseAssistantDetail, assistant: { ...baseAssistantDetail.assistant, id } },
        assistantTypeNameMap: new Map(), assistantTypeCustomField: [], assistantTypeCustomLabel: new Map(),
        assistantTypeCustomTips: new Map(), assistantTypeHideField: [], navigateTo: vi.fn(),
        onConfigChange: vi.fn(), onPromptChange, pluginAssistantFormFields: [], pluginAssistantConfigValues: {},
        onPluginConfigChange: vi.fn(), agentModelOptions: [], agentModelLoading: false, agentModelError: null,
        onAgentProviderChange: vi.fn(), onAgentModelChange: vi.fn(),
    }), { initialProps: { id: 1 } });
    const promptHint = () => result.current.formConfig.find(item => item.key === "prompt")?.config.hint;
    const view = render(React.createElement(React.Fragment, null, promptHint()));
    await userEvent.click(screen.getByRole("button", { name: "重置为默认提示词" }));
    expect(onPromptChange).toHaveBeenCalledWith(expect.stringContaining("今天是 !cd"));
    rerender({ id: 2 });
    view.rerender(React.createElement(React.Fragment, null, promptHint()));
    expect(screen.queryByRole("button", { name: "重置为默认提示词" })).toBeNull();
});

describe("useAssistantFormConfig ACP MCP option", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_all_feature_config", () => []);
    });

    it("shows manual MCP selector for ACP assistants", async () => {
        const onConfigChange = vi.fn();
        const acpAssistant: AssistantDetail = {
            ...baseAssistantDetail,
            assistant: {
                ...baseAssistantDetail.assistant,
                assistant_type: 4,
            },
            model_configs: [],
        };

        const { result } = renderHook(() =>
            useAssistantFormConfig({
                currentAssistant: acpAssistant,
                assistantTypeNameMap: new Map([[4, "Agent 助手"]]),
                assistantTypeCustomField: [],
                assistantTypeCustomLabel: new Map(),
                assistantTypeCustomTips: new Map(),
                assistantTypeHideField: [],
                navigateTo: vi.fn(),
                onConfigChange,
                onPromptChange: vi.fn(),
                pluginAssistantFormFields: [],
                pluginAssistantConfigValues: {},
                onPluginConfigChange: vi.fn(),
                agentModelOptions: [],
                agentModelLoading: false,
                agentModelError: null,
                onAgentProviderChange: vi.fn(),
                onAgentModelChange: vi.fn(),
            })
        );

        await waitFor(() => {
            expect(result.current.formConfig.length).toBeGreaterThan(0);
        });

        const mcpField = result.current.formConfig.find((item) => item.key === "mcp_config");
        const workingDirectoryField = result.current.formConfig.find(
            (item) => item.key === "acp_working_directory"
        );
        expect(mcpField?.config.label).toBe("MCP工具");
        expect(mcpField?.config.type).toBe("custom");
        expect(workingDirectoryField?.config.type).toBe("folder-picker");
        expect(workingDirectoryField?.config.placeholder).toBe("选择工作目录");
        expect(
            result.current.formConfig.some((item) => item.key === "dynamic_mcp_loading_enabled")
        ).toBe(false);
    });
});

describe("useAssistantFormConfig Codex fields", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_all_feature_config", () => []);
        mockInvokeHandler("get_filtered_providers", () => [
            {
                id: 7,
                name: "Codex",
                api_type: "codex_app_server",
                description: "",
                is_official: true,
                is_enabled: true,
            },
        ]);
    });

    it("shows Codex fields immediately and reports provider changes", async () => {
        const onAgentProviderChange = vi.fn();
        const codexAssistant: AssistantDetail = {
            ...baseAssistantDetail,
            assistant: {
                ...baseAssistantDetail.assistant,
                assistant_type: 4,
            },
            model: [{ ...baseAssistantDetail.model[0], provider_id: 7, model_code: "gpt-configured" }],
            model_configs: [
                {
                    id: 2,
                    assistant_id: 1,
                    assistant_model_id: 1,
                    name: "reasoning_effort",
                    value: "high",
                    value_type: "string",
                },
            ],
        };

        const { result } = renderHook(() =>
            useAssistantFormConfig({
                currentAssistant: codexAssistant,
                assistantTypeNameMap: new Map([[4, "Agent 助手"]]),
                assistantTypeCustomField: [],
                assistantTypeCustomLabel: new Map(),
                assistantTypeCustomTips: new Map(),
                assistantTypeHideField: [],
                navigateTo: vi.fn(),
                onConfigChange: vi.fn(),
                onPromptChange: vi.fn(),
                pluginAssistantFormFields: [],
                pluginAssistantConfigValues: {},
                onPluginConfigChange: vi.fn(),
                agentModelOptions: [
                    {
                        code: "gpt-configured%%7",
                        name: "Configured",
                        provider_id: 7,
                        efforts: ["medium", "high"],
                        default_effort: "high",
                        is_default: true,
                    },
                ],
                agentModelLoading: false,
                agentModelError: null,
                onAgentProviderChange,
                onAgentModelChange: vi.fn(),
            })
        );

        await waitFor(() => {
            expect(result.current.formConfig.some((item) => item.key === "agent_model")).toBe(true);
        });
        expect(result.current.formConfig.some((item) => item.key === "reasoning_effort")).toBe(true);
        expect(result.current.formConfig.some((item) => item.key === "codex_sandbox")).toBe(true);

        result.current.formConfig.find((item) => item.key === "acp_provider")?.config.onChange?.("7");
        expect(onAgentProviderChange).toHaveBeenCalledWith("7", "codex_app_server");
    });
});

describe("native agent model selection", () => {
    it.each(["pi", "cursor", "opencode"])("should expose native models without Codex policies when provider is %s", async (apiType) => {
        clearAllMockHandlers();
        mockInvokeHandler("get_all_feature_config", () => []);
        mockInvokeHandler("get_filtered_providers", () => [{ id: 7, name: apiType, api_type: apiType }]);
        const onAgentModelChange = vi.fn();
        const assistant: AssistantDetail = {
            ...baseAssistantDetail,
            assistant: { ...baseAssistantDetail.assistant, assistant_type: 4 },
            model: [{ ...baseAssistantDetail.model[0], provider_id: 7, model_code: "provider/model" }],
        };
        const { result } = renderHook(() => useAssistantFormConfig({
            currentAssistant: assistant, assistantTypeNameMap: new Map([[4, "Agent"]]),
            assistantTypeCustomField: [], assistantTypeCustomLabel: new Map(), assistantTypeCustomTips: new Map(),
            assistantTypeHideField: [], navigateTo: vi.fn(), onConfigChange: vi.fn(), onPromptChange: vi.fn(),
            pluginAssistantFormFields: [], pluginAssistantConfigValues: {}, onPluginConfigChange: vi.fn(),
            agentModelOptions: [{ code: "provider/model%%7", name: "Native model", provider_id: 7, efforts: [], is_default: true }],
            agentModelLoading: false, agentModelError: null, onAgentProviderChange: vi.fn(), onAgentModelChange,
        }));
        await waitFor(() => expect(result.current.formConfig.some(field => field.key === "agent_model")).toBe(true));
        expect(result.current.formConfig.some(field => field.key === "codex_sandbox")).toBe(false);
        expect(result.current.formConfig.some(field => field.key === "reasoning_effort")).toBe(false);
        expect(result.current.formConfig.find(field => field.key === "acp_additional_args")?.config.disabled).toBe(true);
        result.current.formConfig.find(field => field.key === "agent_model")?.config.onChange?.("provider/model%%7");
        expect(onAgentModelChange).toHaveBeenCalledWith("provider/model%%7", "");
    });
});

describe("useAssistantFormConfig tool review mode", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        mockInvokeHandler("get_all_feature_config", () => []);
    });

    it("should report auto_model when the review mode select changes", async () => {
        const onConfigChange = vi.fn();
        const { result } = renderHook(() =>
            useAssistantFormConfig({
                currentAssistant: baseAssistantDetail,
                assistantTypeNameMap: new Map([[0, "普通对话助手"]]),
                assistantTypeCustomField: [],
                assistantTypeCustomLabel: new Map(),
                assistantTypeCustomTips: new Map(),
                assistantTypeHideField: [],
                navigateTo: vi.fn(),
                onConfigChange,
                onPromptChange: vi.fn(),
                pluginAssistantFormFields: [],
                pluginAssistantConfigValues: {},
                onPluginConfigChange: vi.fn(),
                agentModelOptions: [],
                agentModelLoading: false,
                agentModelError: null,
                onAgentProviderChange: vi.fn(),
                onAgentModelChange: vi.fn(),
            })
        );

        await waitFor(() => {
            expect(result.current.formConfig.some((item) => item.key === "tool_review_mode")).toBe(true);
        });

        const reviewField = result.current.formConfig.find((item) => item.key === "tool_review_mode");
        expect(reviewField?.config.value).toBe("off");
        expect(reviewField?.config.options).toEqual([
            { value: "off", label: "人工审核" },
            { value: "auto_model", label: "模型自动审核" },
        ]);
        expect(result.current.formConfig.some((item) => item.key === "tool_review_model_hint")).toBe(false);
        // 过滤 raw model_configs，只保留意图明确的 select，不重复渲染
        expect(result.current.formConfig.filter((item) => item.key === "tool_review_mode")).toHaveLength(1);

        reviewField?.config.onChange?.("auto_model");
        expect(onConfigChange).toHaveBeenCalledWith("tool_review_mode", "auto_model", "string");
    });

    it("should offer a jump to auto review model settings when model review is selected and no model is configured", async () => {
        const assistant: AssistantDetail = {
            ...baseAssistantDetail,
            model_configs: [
                {
                    id: 3,
                    assistant_id: 1,
                    assistant_model_id: 1,
                    name: "tool_review_mode",
                    value: "auto_model",
                    value_type: "string",
                },
            ],
        };
        const { result } = renderHook(() =>
            useAssistantFormConfig({
                currentAssistant: assistant,
                assistantTypeNameMap: new Map([[0, "普通对话助手"]]),
                assistantTypeCustomField: [],
                assistantTypeCustomLabel: new Map(),
                assistantTypeCustomTips: new Map(),
                assistantTypeHideField: [],
                navigateTo: vi.fn(),
                onConfigChange: vi.fn(),
                onPromptChange: vi.fn(),
                pluginAssistantFormFields: [],
                pluginAssistantConfigValues: {},
                onPluginConfigChange: vi.fn(),
                agentModelOptions: [],
                agentModelLoading: false,
                agentModelError: null,
                onAgentProviderChange: vi.fn(),
                onAgentModelChange: vi.fn(),
            })
        );

        await waitFor(() => {
            expect(result.current.formConfig.some((item) => item.key === "tool_review_model_hint")).toBe(true);
        });
        expect(result.current.formConfig.filter((item) => item.key === "tool_review_mode")).toHaveLength(1);

        const hint = result.current.formConfig.find((item) => item.key === "tool_review_model_hint");
        render(React.createElement(React.Fragment, null, hint?.config.customRender?.()));
        const user = userEvent.setup();
        await user.click(screen.getByRole("button", { name: "前往配置自动审核模型" }));
        expect(emit).toHaveBeenCalledWith("config-navigate-to", {
            menu: "feature-assistant-config",
            subNav: "conversation_summary#auto-review-model",
        });
    });
});

it("should group legacy stored plugin fields as extensions without changing their values or callbacks", () => {
    const onConfigChange = vi.fn();
    const configs = [
        { name: "confirmBeforeGenerate", value: "true", value_type: "boolean" },
        { name: "fileScanDirectory", value: "E:/example/project", value_type: "string" },
        { name: "temperature", value: "0.7", value_type: "float" },
    ].map((config, id) => ({ ...config, id, assistant_id: 1, assistant_model_id: 1 }));
    const { result } = renderHook(() => useAssistantFormConfig({
        currentAssistant: { ...baseAssistantDetail, model_configs: configs },
        assistantTypeNameMap: new Map(), assistantTypeCustomField: [], assistantTypeCustomLabel: new Map(),
        assistantTypeCustomTips: new Map(), assistantTypeHideField: [], navigateTo: vi.fn(),
        onConfigChange, onPromptChange: vi.fn(), pluginAssistantFormFields: [], pluginAssistantConfigValues: {},
        onPluginConfigChange: vi.fn(), agentModelOptions: [], agentModelLoading: false, agentModelError: null,
        onAgentProviderChange: vi.fn(), onAgentModelChange: vi.fn(),
    }));
    const field = (key: string) => result.current.formConfig.find(item => item.key === key)!;
    expect(field("confirmBeforeGenerate").section).toBe("extension");
    expect(field("confirmBeforeGenerate").config.value).toBe(true);
    expect(field("fileScanDirectory").section).toBe("extension");
    expect(field("fileScanDirectory").config.value).toBe("E:/example/project");
    expect(field("temperature").section).toBeUndefined();
    expect(field("model").section).toBeUndefined();
    field("confirmBeforeGenerate").config.onChange?.(false);
    expect(onConfigChange).toHaveBeenCalledWith("confirmBeforeGenerate", false, "boolean");
});
