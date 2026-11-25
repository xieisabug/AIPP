import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useForm } from "react-hook-form";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { AssistantFormRenderer, classifyAssistantFieldTab } from "./AssistantFormRenderer";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => true }));
vi.mock("@/hooks/feature/useFeatureConfig", () => ({
    useFeatureConfig: () => ({ featureConfig: new Map(), loading: false }),
}));
vi.mock("@/hooks/useFilteredModels", () => ({
    useFilteredModels: () => ({ models: [], loading: false, error: null }),
}));
vi.mock("@/hooks/useFilteredProviders", () => ({
    useFilteredProviders: () => ({ providers: [], loading: false, error: null }),
}));
vi.mock("@/hooks/useMcpServers", () => ({
    useMcpServers: () => ({ mcpServers: [], loading: false, error: null }),
}));

function Wrapper(props: Omit<React.ComponentProps<typeof AssistantFormRenderer>, "form">) {
    const form = useForm();
    return <AssistantFormRenderer {...props} form={form} />;
}

const baseProps = {
    currentAssistant: {
        assistant: { id: 5, name: "Test assistant", description: "saved", assistant_type: 2 },
        model: [],
        model_configs: [],
        prompts: [],
    },
    formConfig: [],
    assistantConfigApi: { clearFieldValue: vi.fn(), changeFieldValue: vi.fn() },
    onSave: vi.fn(),
} as unknown as Omit<React.ComponentProps<typeof AssistantFormRenderer>, "form">;

describe("AssistantFormRenderer 5-tab classification and mobile capability", () => {
    beforeEach(() => clearAllMockHandlers());

    it("classifies assistant fields into role, tools, permissions, and advanced tabs", () => {
        expect(classifyAssistantFieldTab({ key: "prompt", config: { type: "textarea", label: "Prompt" } })).toBe("role");
        expect(classifyAssistantFieldTab({ key: "mcp_config", config: { type: "custom", label: "MCP" } })).toBe("tools");
        expect(classifyAssistantFieldTab({ key: "skills_config", config: { type: "custom", label: "Skills" } })).toBe("tools");
        expect(classifyAssistantFieldTab({ key: "workspace_config", config: { type: "custom", label: "Workspace" } })).toBe("permissions");
        expect(classifyAssistantFieldTab({ key: "tool_review_mode", config: { type: "select", label: "Review" } })).toBe("permissions");
        expect(classifyAssistantFieldTab({ key: "temperature", config: { type: "input", label: "Temp" } })).toBe("advanced");
        expect(
            classifyAssistantFieldTab({
                key: "plugin::1::enabled",
                section: "extension",
                config: { type: "checkbox", label: "Plugin" },
            })
        ).toBe("advanced");
    });

    it.each([0, 4])("renders the 5-tab layout for compatible assistant type %s", async (assistantType) => {
        mockInvokeHandler("get_assistant_execution_capability", () => ({ available: true, issues: [] }));
        render(
            <Wrapper
                {...baseProps}
                currentAssistant={{
                    ...baseProps.currentAssistant!,
                    assistant: { ...baseProps.currentAssistant!.assistant, assistant_type: assistantType },
                }}
                formConfig={[
                    { key: "mcp_config", config: { type: "custom", label: "MCP", customRender: () => <div>mcp</div> } },
                    { key: "skills_config", config: { type: "custom", label: "Skills", customRender: () => <div>skills</div> } },
                    { key: "plugin::1::enabled", section: "extension", config: { type: "checkbox", label: "Plugin" } },
                ]}
            />
        );
        expect(await screen.findByRole("tab", { name: /角色与模型/ })).toBeTruthy();
        expect(screen.getByRole("tab", { name: /能力与工具/ })).toBeTruthy();
        expect(screen.getByRole("tab", { name: /权限与工作区/ })).toBeTruthy();
        expect(screen.getByRole("tab", { name: /参数与高级/ })).toBeTruthy();
        expect(screen.getByRole("tab", { name: /记忆/ })).toBeTruthy();
    });

    it("keeps an incompatible assistant read-only", async () => {
        mockInvokeHandler("get_assistant_execution_capability", () => ({
            available: false,
            issues: [{ capability: "local_cli_agent", reason: "desktop only" }],
        }));
        render(<Wrapper {...baseProps} />);
        expect(await screen.findByText("desktop only")).toBeTruthy();
        expect(screen.getByText(/移动端只读/)).toBeTruthy();
        expect(screen.queryByRole("tab", { name: /角色与模型/ })).toBeNull();
    });

    it("shows the failure and keeps the form read-only if capability lookup fails", async () => {
        mockInvokeHandler("get_assistant_execution_capability", () => {
            throw new Error("capability offline");
        });
        render(<Wrapper {...baseProps} />);
        expect(await screen.findByText(/能力校验失败.*capability offline/)).toBeTruthy();
        expect(screen.getByText(/移动端只读/)).toBeTruthy();
        expect(screen.queryByRole("tab", { name: /角色与模型/ })).toBeNull();
    });
});
