import type { ReactNode } from "react";
import type { UseFormReturn } from "react-hook-form";
import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import AssistantConfig from "./AssistantConfig";
import type { AssistantDetail } from "@/data/Assistant";
import type { LoadedPlugin } from "@/services/PluginRuntime";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";

vi.mock("@/components/common", () => ({
    ConfigPageLayout: ({ sidebar, content }: { sidebar: ReactNode; content: ReactNode }) => <>{sidebar}{content}</>,
    SidebarList: ({ children }: { children: ReactNode }) => <>{children}</>,
    ListItemButton: ({ children, onClick }: { children: ReactNode; onClick: () => void }) => <button onClick={onClick}>{children}</button>,
    EmptyState: () => <div>empty</div>,
}));
vi.mock("./AddAssistantDialog", () => ({ default: () => null }));
vi.mock("./assistant/AssistantDialogs", () => ({ AssistantDialogs: () => null }));
vi.mock("@/hooks/assistant/useAssistantFormConfig", () => ({
    useAssistantFormConfig: ({ pluginAssistantConfigValues }: { pluginAssistantConfigValues: Record<string, unknown> }) => ({ formConfig: pluginAssistantConfigValues }),
}));
vi.mock("./assistant/AssistantFormRenderer", () => ({
    AssistantFormRenderer: ({ currentAssistant, form, formConfig, onCopy }: {
        currentAssistant: AssistantDetail; form: UseFormReturn; formConfig: Record<string, unknown>; onCopy?: () => void;
    }) => <>
        <div data-testid="selected">{currentAssistant.assistant.id}</div>
        <input aria-label="prompt" {...form.register("prompt")} />
        <input aria-label="plugin" {...form.register("plugin::8::note")} />
        <div data-testid="plugin-state">{String(formConfig["plugin::8::note"] ?? "")}</div>
        <button onClick={onCopy}>copy</button>
    </>,
}));

const plugins = [{ pluginId: 8, code: "test", name: "test", pluginType: [],
    contributions: { assistantFormFields: [{ key: "note", type: "text", label: "note" }] },
}] as unknown as LoadedPlugin[];
const detail = (id: number): AssistantDetail => ({
    assistant: { id, name: "assistant-" + id, assistant_type: 0, description: null, is_addition: false, created_time: "now" },
    prompts: [{ id, assistant_id: id, prompt: "prompt-" + id, created_time: "now" }],
    model: [], model_configs: [], prompt_params: [], mcp_configs: [], mcp_tool_configs: [],
});
const config = (id: number) => [{ configId: id, pluginId: 8, assistantId: id, configKey: "note", configValue: "plugin-" + id }];

function setup() {
    let ids = [1, 2, 3];
    let resolve!: (value: ReturnType<typeof config>) => void;
    const pending = new Promise<ReturnType<typeof config>>((done) => { resolve = done; });
    mockInvokeHandler("get_assistants", () => ids.map((id) => detail(id).assistant));
    mockInvokeHandler("get_assistant", (args) => detail(Number(args?.assistantId)));
    mockInvokeHandler("get_plugin_assistant_configs", (args) => Number(args?.assistantId) === 2 ? pending : config(Number(args?.assistantId)));
    render(<AssistantConfig pluginList={plugins} navigateTo={() => undefined} />);
    return { removeFirst: () => { ids = [2, 3]; }, resolve };
}

describe("AssistantConfig sync selection", () => {
    afterEach(() => { clearAllMockHandlers(); vi.clearAllMocks(); });

    it("should keep copied assistant form when an older refresh finishes", async () => {
        const state = setup();
        mockInvokeHandler("copy_assistant", () => detail(4));
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        state.removeFirst();
        await act(async () => { await emit("assistant_list_changed"); });
        await waitFor(() => expect(screen.getByTestId("selected")).toHaveTextContent("2"));
        fireEvent.click(screen.getByRole("button", { name: "copy" }));
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-4"));
        await act(async () => { state.resolve(config(2)); });
        expect(screen.getByTestId("selected")).toHaveTextContent("4");
        expect(screen.getByLabelText("prompt")).toHaveValue("prompt-4");
        expect(screen.getByLabelText("plugin")).toHaveValue("plugin-4");
    });

    it("should finish resetting fallback form when plugin values arrive after selection changes", async () => {
        const state = setup();
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        state.removeFirst();
        await act(async () => { await emit("assistant_list_changed"); });
        await waitFor(() => expect(screen.getByTestId("selected")).toHaveTextContent("2"));
        await act(async () => { state.resolve(config(2)); });
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-2"));
        expect(screen.getByLabelText("plugin")).toHaveValue("plugin-2");
        expect(screen.getByTestId("plugin-state")).toHaveTextContent("plugin-2");
    });

    it("should discard delayed fallback values when user selects another assistant", async () => {
        const state = setup();
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        state.removeFirst();
        await act(async () => { await emit("assistant_list_changed"); });
        await waitFor(() => expect(screen.getByTestId("selected")).toHaveTextContent("2"));
        fireEvent.click(screen.getByRole("button", { name: "assistant-3" }));
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-3"));
        await act(async () => { state.resolve(config(2)); });
        expect(screen.getByTestId("selected")).toHaveTextContent("3");
        expect(screen.getByLabelText("prompt")).toHaveValue("prompt-3");
        expect(screen.getByLabelText("plugin")).toHaveValue("plugin-3");
        expect(screen.getByTestId("plugin-state")).toHaveTextContent("plugin-3");
    });
});
