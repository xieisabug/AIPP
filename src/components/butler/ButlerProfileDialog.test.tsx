import { fireEvent, render, screen, waitFor, cleanup } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import userEvent from "@testing-library/user-event";
import ButlerProfileDialog from "./ButlerProfileDialog";

const memoryApi = vi.hoisted(() => ({
    listButlerMemories: vi.fn(), updateButlerMemory: vi.fn(), deleteButlerMemory: vi.fn(),
    getButlerMemorySource: vi.fn(), onButlerMemoryChanged: vi.fn(),
}));
vi.mock("@/services/butlerMemoryService", () => memoryApi);
const chatModels = [{ name: "Old model", code: "old", llm_provider_id: 1, id: 1 }, { name: "New model", code: "new", llm_provider_id: 2, id: 2 }];
// 智能体提供商没有登记模型时，候选只有一条代表提供商本身的选项（id 0、code acp），与后端 get_filtered_models_for_select 一致。
const agentProviders = [{ name: "Codex", code: "acp", llm_provider_id: 39, id: 0 }, { name: "Claude Code", code: "acp", llm_provider_id: 40, id: 0 }];
const modelList = vi.hoisted(() => ({ current: { models: [] as unknown[], loading: false, error: null as string | null } }));
vi.mock("@/hooks/useFilteredModels", () => ({ useFilteredModels: () => modelList.current }));

describe("Butler profile editor", () => {
    beforeEach(() => { modelList.current = { models: chatModels, loading: false, error: null }; });
    afterEach(cleanup);

    it("should edit description and model like the add form while keeping the type read-only", async () => {
        const user = userEvent.setup();
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="小A" description="写作" model={{ assistantType: 0, typeName: "普通对话助手", current: { modelCode: "old", providerId: 1 } }}
            onSave={onSave} onClose={vi.fn()} />);
        expect(screen.getByLabelText("类型")).toHaveValue("普通对话助手");
        expect(screen.getByLabelText("类型")).toBeDisabled();
        fireEvent.change(screen.getByLabelText("描述（选填）"), { target: { value: "写周报" } });
        await user.click(screen.getByRole("combobox", { name: "模型" }));
        await user.click(screen.getByRole("option", { name: "New model" }));
        await user.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("小A", expect.objectContaining({ motion: true }), "写周报", { modelCode: "new", providerId: 2 }));
    });

    it("should show the current model when the model list finishes loading after the dialog opens", () => {
        modelList.current = { models: [], loading: true, error: null };
        const model = { assistantType: 0, typeName: "普通对话助手", current: { modelCode: "new", providerId: 2 } };
        const { rerender } = render(<ButlerProfileDialog name="小A" description="" model={model} onSave={vi.fn()} onClose={vi.fn()} />);
        expect(screen.getByRole("combobox", { name: "模型" })).toHaveTextContent("加载模型中…");
        modelList.current = { models: chatModels, loading: false, error: null };
        rerender(<ButlerProfileDialog name="小A" description="" model={model} onSave={vi.fn()} onClose={vi.fn()} />);
        expect(screen.getByRole("combobox", { name: "模型" })).toHaveTextContent("New model");
        expect(screen.queryByText(/已不可用/)).toBeNull();
    });

    it("should show an agent's provider as its model when the provider has no registered models", () => {
        modelList.current = { models: agentProviders, loading: false, error: null };
        // Codex 的默认模型由其自身配置决定，不在模型表里；未选模型的智能体存的是空模型代码。
        for (const modelCode of ["gpt-6.1-sol", ""]) {
            render(<ButlerProfileDialog name="小B" description="" onSave={vi.fn()} onClose={vi.fn()}
                model={{ assistantType: 4, typeName: "智能体助手", current: { modelCode, providerId: 39 } }} />);
            expect(screen.getByRole("combobox", { name: "模型 / 智能体提供商" })).toHaveTextContent("Codex");
            expect(screen.queryByText(/已不可用/)).toBeNull();
            cleanup();
        }
    });

    it("should save an empty model code when an agent switches to another provider", async () => {
        const user = userEvent.setup();
        modelList.current = { models: agentProviders, loading: false, error: null };
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="小B" description="" onSave={onSave} onClose={vi.fn()}
            model={{ assistantType: 4, typeName: "智能体助手", current: { modelCode: "gpt-6.1-sol", providerId: 39 } }} />);
        await user.click(screen.getByRole("combobox", { name: "模型 / 智能体提供商" }));
        await user.click(screen.getByRole("option", { name: "Claude Code" }));
        await user.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("小B", expect.anything(), "", { modelCode: "", providerId: 40 }));
    });

    it("should leave the model untouched when the selection is unchanged", async () => {
        const user = userEvent.setup();
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="小A" description="" onSave={onSave} onClose={vi.fn()}
            model={{ assistantType: 0, typeName: "普通对话助手", current: { modelCode: "old", providerId: 1 } }} />);
        await user.click(screen.getByRole("combobox", { name: "模型" }));
        await user.click(screen.getByRole("option", { name: "New model" }));
        await user.click(screen.getByRole("combobox", { name: "模型" }));
        await user.click(screen.getByRole("option", { name: "Old model" }));
        await user.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("小A", expect.anything(), "", undefined));
    });

    it("should list the assistant's memories in a separate tab when a conversation is given", async () => {
        memoryApi.listButlerMemories.mockResolvedValue([{ id: 5, content: "用户希望回答先给结论", scope_type: "assistant", scope_id: 2,
            author_assistant_id: 2, source_message_id: 9, source_group_id: null, source_group_seq: null, revision: 1, created_at: 0, updated_at: 0 }]);
        memoryApi.onButlerMemoryChanged.mockResolvedValue(() => {});
        render(<ButlerProfileDialog name="小A" memoryConversationId={20} onSave={vi.fn()} onClose={vi.fn()} />);
        expect(memoryApi.listButlerMemories).not.toHaveBeenCalled();
        fireEvent.mouseDown(screen.getByRole("tab", { name: "记忆" }), { button: 0 });
        expect(await screen.findByText("用户希望回答先给结论")).toBeInTheDocument();
        expect(memoryApi.listButlerMemories).toHaveBeenCalledWith(20);
    });

    it("should save trimmed name and independent colors when confirming the preview", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={onSave} onClose={onClose} />);
        fireEvent.change(screen.getByLabelText("名称"), { target: { value: " 小管家 " } });
        fireEvent.click(screen.getByRole("button", { name: "左侧 · 亮色：薄荷绿" }));
        fireEvent.click(screen.getByRole("button", { name: "右侧 · 深色：海军蓝" }));
        expect(onSave).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onClose).toHaveBeenCalledOnce());
        expect(onSave).toHaveBeenCalledWith("小管家", {
            version: 2, shape: "aipp", colors: { primary: "#CEF3DF", secondary: "#193B68" },
            accessories: { eyes: null, head: null }, motion: true,
        });
    });

    it("should discard edits when cancelled and reject blank names", () => {
        const onSave = vi.fn();
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={onSave} onClose={onClose} />);
        fireEvent.change(screen.getByLabelText("名称"), { target: { value: "  " } });
        expect(screen.getByRole("button", { name: "保存" })).toBeDisabled();
        fireEvent.click(screen.getByRole("button", { name: "取消" }));
        expect(onSave).not.toHaveBeenCalled();
        expect(onClose).toHaveBeenCalledOnce();
    });

    it("should preserve draft and show the underlying error when saving fails", async () => {
        const onClose = vi.fn();
        render(<ButlerProfileDialog name="总管家" onSave={vi.fn().mockRejectedValue(new Error("配置写入失败：database is locked"))} onClose={onClose} />);
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("database is locked");
        expect(screen.getByLabelText("名称")).toHaveValue("总管家");
        expect(onClose).not.toHaveBeenCalled();
    });

    it("should offer one body palette and keep decorations when saving the pixel shape", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="助手" onSave={onSave} onClose={vi.fn()} />);
        const click = (name: string) => fireEvent.click(screen.getByRole("button", { name }));
        click("脸部：腮红");
        click("头部：小花");
        click("像素仔");
        expect(screen.queryByRole("button", { name: /亮色：/ })).toBeNull();
        expect(screen.getByRole("button", { name: "身体 · 深色：纯黑" })).toHaveAttribute("aria-pressed", "true");
        click("身体 · 深色：陶橘");
        for (const [shape, area] of [["圆团", "外圈"], ["AIPP", "右侧"], ["像素仔", "身体"]]) {
            click(shape);
            expect(screen.getByRole("button", { name: `${area} · 深色：陶橘` })).toHaveAttribute("aria-pressed", "true");
        }
        click("恢复黑色");
        expect(document.querySelectorAll('[data-aipp-slot="assistant-profile-preview"] path[fill="#FFFFFF"][stroke="#000000"]')).toHaveLength(2);
        click("保存");
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("助手", expect.objectContaining({
            shape: "pixel", colors: { primary: "#FFFFFF", secondary: "#000000" },
            accessories: { eyes: "blush", head: "flower" },
        })));
    });

    it.each([["棒球帽", "baseball-cap"], ["反向棒球帽", "baseball-cap-left"]])("should retain %s when changing shape and restoring colors", async (label, id) => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="助手" onSave={onSave} onClose={vi.fn()} />);
        fireEvent.click(screen.getByRole("button", { name: "脸部：圆框眼镜" }));
        fireEvent.click(screen.getByRole("button", { name: `头部：${label}` }));
        fireEvent.click(screen.getByRole("button", { name: "圆团" }));
        fireEvent.click(screen.getByRole("button", { name: "脸部 · 亮色：蜜桃" }));
        fireEvent.click(screen.getByRole("button", { name: "恢复黑白配色" }));
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("助手", {
            version: 2, shape: "pebble", colors: { primary: "#FFFFFF", secondary: "#000000" },
            accessories: { eyes: "round-glasses", head: id }, motion: true,
        }));
    });

    it("should offer face and head decorations without manual preview controls", async () => {
        const onSave = vi.fn().mockResolvedValue(undefined);
        render(<ButlerProfileDialog name="助手" appearance={{ version: 2, shape: "aipp", colors: { primary: "#FAD4E4", secondary: "#000000" },
            accessories: { eyes: null, head: null }, motion: false }} onSave={onSave} onClose={vi.fn()} />);
        expect(screen.queryByRole("checkbox")).toBeNull();
        expect(screen.queryByRole("combobox")).toBeNull();
        fireEvent.click(screen.getByRole("button", { name: "脸部：墨镜" }));
        fireEvent.click(screen.getByRole("button", { name: "头部：铃铛蝴蝶结" }));
        expect(screen.getByRole("button", { name: "头部：铃铛蝴蝶结" })).toHaveAttribute("aria-pressed", "true");
        fireEvent.click(screen.getByRole("button", { name: "保存" }));
        await waitFor(() => expect(onSave).toHaveBeenCalledWith("助手", {
            version: 2, shape: "aipp", colors: { primary: "#FAD4E4", secondary: "#000000" },
            accessories: { eyes: "sunglasses", head: "bow-bell" }, motion: true,
        }));
    });
});
