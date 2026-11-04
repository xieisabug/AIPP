import { renderHook, waitFor } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import type { ReactElement } from "react";
import type { AssistantDetail } from "@/data/Assistant";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import MobileSkillsManager from "@/components/config/MobileSkillsManager";
import { useAssistantFormConfig } from "./useAssistantFormConfig";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => true, usePlatform: () => "android" }));

it("should expose mobile Skills binding for the selected Android assistant", async () => {
    clearAllMockHandlers();
    mockInvokeHandler("get_all_feature_config", () => []);
    const assistant = {
        assistant: { id: 3, name: "Fixture", assistant_type: 0 },
        prompts: [], model: [], model_configs: [], prompt_params: [], mcp_configs: [], mcp_tool_configs: [],
    } as unknown as AssistantDetail;
    const { result } = renderHook(() => useAssistantFormConfig({
        currentAssistant: assistant, assistantTypeNameMap: new Map([[0, "普通对话助手"]]),
        assistantTypeCustomField: [], assistantTypeCustomLabel: new Map(), assistantTypeCustomTips: new Map(), assistantTypeHideField: [],
        navigateTo: vi.fn(), onConfigChange: vi.fn(), onPromptChange: vi.fn(),
        pluginAssistantFormFields: [], pluginAssistantConfigValues: {}, onPluginConfigChange: vi.fn(),
        agentModelOptions: [], agentModelLoading: false, agentModelError: null,
        onAgentProviderChange: vi.fn(), onAgentModelChange: vi.fn(),
    }));
    await waitFor(() => expect(result.current.formConfig.some(field => field.key === "skills_config")).toBe(true));
    const config = result.current.formConfig.find(field => field.key === "skills_config")!.config;
    const rendered = config.customRender?.() as ReactElement<{ assistantId: number }>;
    expect(rendered.type).toBe(MobileSkillsManager);
    expect(rendered.props.assistantId).toBe(3);
});
