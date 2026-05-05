import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

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
                assistantTypeNameMap: new Map([[4, "ACP 助手"]]),
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
            })
        );

        await waitFor(() => {
            expect(result.current.formConfig.length).toBeGreaterThan(0);
        });

        const mcpField = result.current.formConfig.find((item) => item.key === "mcp_config");
        expect(mcpField?.config.label).toBe("MCP工具");
        expect(mcpField?.config.type).toBe("custom");
        expect(
            result.current.formConfig.some((item) => item.key === "dynamic_mcp_loading_enabled")
        ).toBe(false);
    });
});
