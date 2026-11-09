import type { ReactNode } from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AssistantConfig from "./AssistantConfig";
import type { AssistantDetail } from "@/data/Assistant";
import type { LoadedPlugin } from "@/services/PluginRuntime";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import { requestConfigLeave } from "@/lib/configLeaveGuard";

vi.mock("@/components/common", () => ({
    ConfigPageLayout: ({ sidebar, content }: { sidebar: ReactNode; content: ReactNode }) => <>{sidebar}{content}</>,
    SidebarList: ({ children }: { children: ReactNode }) => <>{children}</>,
    ListItemButton: ({ children, onClick }: { children: ReactNode; onClick: () => void }) => <button onClick={onClick}>{children}</button>,
    EmptyState: () => <div>empty</div>,
    ConfigPageLoading: () => <div>loading</div>,
}));
vi.mock("./AddAssistantDialog", () => ({ default: () => null }));
vi.mock("./assistant/AssistantDialogs", () => ({ AssistantDialogs: () => null }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false, usePlatform: () => "windows" }));
vi.mock("./AssistantMCPFieldDisplay", () => ({ default: () => <span>MCP</span> }));
vi.mock("./AssistantSkillsFieldDisplay", () => ({ default: () => <span>Skills</span> }));
vi.mock("./AssistantWorkspaceFieldDisplay", () => ({ default: () => <span>Workspace</span> }));
// A plugin with async settings makes the gap between detail and form loading observable.
const plugins = [{ pluginId: 1, code: "test", name: "test", pluginType: [],
    contributions: { assistantFormFields: [
        { key: "enabled", label: "启用插件", type: "checkbox", defaultValue: false },
        { key: "role", label: "注入角色", type: "select", defaultValue: "system",
            options: [{ value: "system", label: "System" }, { value: "user", label: "User" }] },
    ] },
}] as unknown as LoadedPlugin[];
const detail = (id: number): AssistantDetail => ({
    assistant: { id, name: "assistant-" + id, assistant_type: 0, description: null, is_addition: false, created_time: "now" },
    prompts: [{ id, assistant_id: id, prompt: "prompt-" + id, created_time: "now" }],
    model: [{ id, assistant_id: id, model_code: id === 1 ? "test" : "", provider_id: id === 1 ? 1 : 0, alias: "" }],
    model_configs: Object.entries({ ...(id === 1 ? { tool_review_mode: ["auto_model", "string"] } : {}), dynamic_mcp_loading_enabled: ["true", "boolean"], max_tokens: ["32000", "number"], reasoning_effort: [id === 1 ? "medium" : "", "string"], stream: ["true", "boolean"], temperature: ["", "float"], top_p: ["", "float"], use_native_toolcall: ["true", "boolean"] }).map(([name, [value, value_type]], index) => ({ id: index, assistant_id: id, assistant_model_id: id, name, value, value_type })),
    prompt_params: [], mcp_configs: [], mcp_tool_configs: [],
});
describe("AssistantConfig real form initialization", () => {
    afterEach(() => { clearAllMockHandlers(); vi.clearAllMocks(); });
    it("should remain clean when opening and expanding an untouched assistant", async () => {
        mockInvokeHandler("get_assistants", () => [detail(1).assistant, detail(2).assistant]);
        mockInvokeHandler("get_assistant", args => detail(Number(args?.assistantId)));
        let resolvePlugin!: (value: unknown[]) => void;
        const pendingPlugin = new Promise<unknown[]>(resolve => { resolvePlugin = resolve; });
        mockInvokeHandler("get_plugin_assistant_configs", args => Number(args?.assistantId) === 2 ? pendingPlugin : []);
        mockInvokeHandler("get_all_feature_config", () => [{ id: 1, feature_code: "experimental", key: "dynamic_mcp_loading_enabled", value: "true" }]);
        mockInvokeHandler("get_filtered_models_for_select", () => [{ id: 1, name: "Test", code: "test", llm_provider_id: 1 }]);
        render(<AssistantConfig pluginList={plugins} navigateTo={() => undefined} />);
        await screen.findByText("高级配置");
        await act(async () => { await new Promise(resolve => setTimeout(resolve, 100)); });
        expect(screen.queryByText("未保存")).toBeNull();
        fireEvent.click(screen.getByText("高级配置"));
        fireEvent.click(screen.getByText("扩展配置"));
        fireEvent.click(screen.getByRole("button", { name: "assistant-2" }));
        await screen.findByText("正在加载助手配置…");
        expect(screen.queryByText("高级配置")).toBeNull();
        await act(async () => { resolvePlugin([]); });
        await screen.findByRole("button", { name: "配置你的智能助手" });
        fireEvent.click(screen.getByText("高级配置"));
        expect(screen.getByText("人工审核")).toBeInTheDocument();
        await act(async () => { await new Promise(resolve => setTimeout(resolve, 100)); });
        const proceed = vi.fn();
        act(() => { requestConfigLeave(proceed); });
        expect(proceed).toHaveBeenCalledOnce();
        expect(screen.queryByText("有未保存的助手配置")).toBeNull();
        fireEvent.change(screen.getByDisplayValue("prompt-2"), { target: { value: "edited" } });
        const leaveAfterEdit = vi.fn();
        act(() => { requestConfigLeave(leaveAfterEdit); });
        expect(leaveAfterEdit).not.toHaveBeenCalled();
        expect(screen.getByText("有未保存的助手配置")).toBeInTheDocument();
    });
});
