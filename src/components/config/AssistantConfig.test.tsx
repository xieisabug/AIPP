import type { ReactNode } from "react";
import { Controller, type UseFormReturn } from "react-hook-form";
import { emit } from "@tauri-apps/api/event";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
vi.mock("@/hooks/assistant/useAssistantFormConfig", () => ({
    useAssistantFormConfig: ({ pluginAssistantConfigValues }: { pluginAssistantConfigValues: Record<string, unknown> }) => ({ formConfig: pluginAssistantConfigValues }),
}));
vi.mock("./assistant/AssistantFormRenderer", () => ({
    // Mirror production Controller-driven inputs so reset()/setValue re-render the DOM.
    AssistantFormRenderer: ({ currentAssistant, form, formConfig, onCopy, onSave }: {
        currentAssistant: AssistantDetail; form: UseFormReturn; formConfig: Record<string, unknown>; onCopy?: () => void; onSave?: () => void;
    }) => <>
        <div data-testid="selected">{currentAssistant.assistant.id}</div>
        <Controller
            control={form.control}
            name="prompt"
            render={({ field }) => <input aria-label="prompt" {...field} value={field.value ?? ""} />}
        />
        <Controller
            control={form.control}
            name="plugin::8::note"
            render={({ field }) => <input aria-label="plugin" {...field} value={field.value ?? ""} />}
        />
        <div data-testid="plugin-state">{String(formConfig["plugin::8::note"] ?? "")}</div>
        <button onClick={onCopy}>copy</button>
        <button type="button" onClick={() => { void onSave?.(); }}>保存配置</button>
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

    it("preserves a dirty prompt on external refresh while allowing explicit selection", async () => {
        setup();
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        fireEvent.change(screen.getByLabelText("prompt"), { target: { value: "unsaved" } });
        await act(async () => { await emit("mcp_state_changed"); });
        await act(async () => { await new Promise(resolve => setTimeout(resolve, 150)); });
        expect(screen.getByLabelText("prompt")).toHaveValue("unsaved");
        expect(screen.getByTestId("selected")).toHaveTextContent("1");
        fireEvent.click(screen.getByRole("button", { name: "assistant-3" }));
        // #157: dirty leave prompts save/discard instead of silently dropping edits
        await waitFor(() => expect(screen.getByText("有未保存的助手配置")).toBeInTheDocument());
        fireEvent.click(screen.getByRole("button", { name: "放弃" }));
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-3"));
    });

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

describe("AssistantConfig leave guard", () => {
    afterEach(() => { clearAllMockHandlers(); vi.clearAllMocks(); });

    it("awaits async save before proceeding to another assistant", async () => {
        // Avoid setup()'s forever-pending plugin configs for id=2 so choose can finish.
        let ids = [1, 2, 3];
        mockInvokeHandler("get_assistants", () => ids.map((id) => detail(id).assistant));
        mockInvokeHandler("get_assistant", (args) => detail(Number(args?.assistantId)));
        mockInvokeHandler("get_plugin_assistant_configs", (args) => config(Number(args?.assistantId)));
        render(<AssistantConfig pluginList={plugins} navigateTo={() => undefined} />);
        let resolveSave!: () => void;
        const saveGate = new Promise<void>((done) => { resolveSave = done; });
        const saveCalls: unknown[] = [];
        mockInvokeHandler("save_assistant", async (args) => {
            saveCalls.push(args);
            await saveGate;
        });
        mockInvokeHandler("set_plugin_assistant_config", () => undefined);
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        fireEvent.change(screen.getByLabelText("prompt"), { target: { value: "edited-before-leave" } });
        fireEvent.click(screen.getByRole("button", { name: "assistant-2" }));
        await waitFor(() => expect(screen.getByText("有未保存的助手配置")).toBeInTheDocument());
        expect(screen.getAllByText("有未保存的助手配置")).toHaveLength(1);
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        // Save kicked off but proceed must wait — still on assistant 1
        await waitFor(() => expect(saveCalls.length).toBe(1));
        expect(screen.getByTestId("selected")).toHaveTextContent("1");
        expect(screen.getByLabelText("prompt")).toHaveValue("edited-before-leave");
        await act(async () => { resolveSave(); });
        await waitFor(() => expect(screen.getByTestId("selected")).toHaveTextContent("2"));
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-2"));
    });

    it("resets dirty fields to the loaded snapshot on discard (keep-alive safe)", async () => {
        setup();
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        fireEvent.change(screen.getByLabelText("prompt"), { target: { value: "unsaved-edit" } });
        expect(screen.getByLabelText("prompt")).toHaveValue("unsaved-edit");
        let left = false;
        act(() => {
            requestConfigLeave(() => { left = true; });
        });
        await waitFor(() => expect(screen.getByText("有未保存的助手配置")).toBeInTheDocument());
        expect(screen.getAllByRole("alertdialog")).toHaveLength(1);
        fireEvent.click(screen.getByRole("button", { name: "放弃" }));
        await waitFor(() => expect(left).toBe(true));
        // Snapshot restore — not empty useForm defaults
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        expect(screen.getByTestId("selected")).toHaveTextContent("1");
    });

    it("does not proceed when leave-save fails", async () => {
        setup();
        mockInvokeHandler("save_assistant", async () => {
            throw new Error("boom");
        });
        await waitFor(() => expect(screen.getByLabelText("prompt")).toHaveValue("prompt-1"));
        fireEvent.change(screen.getByLabelText("prompt"), { target: { value: "will-fail-save" } });
        fireEvent.click(screen.getByRole("button", { name: "assistant-3" }));
        await waitFor(() => expect(screen.getByText("有未保存的助手配置")).toBeInTheDocument());
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(screen.getByText("有未保存的助手配置")).toBeInTheDocument());
        expect(screen.getByTestId("selected")).toHaveTextContent("1");
        expect(screen.getByLabelText("prompt")).toHaveValue("will-fail-save");
    });
});
