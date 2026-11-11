import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import AddAssistantDialog from "./AddAssistantDialog";
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));
vi.mock("@/hooks/useFilteredModels", () => ({ useFilteredModels: (type: number) => ({
    models: type === 4 ? [{ name: "Agent provider", code: "acp", llm_provider_id: 20 }] : [{ name: "Test model", code: "test", llm_provider_id: 10 }], loading: false, error: null,
}) }));
describe("AddAssistantDialog model selection", () => {
    afterEach(clearAllMockHandlers);
    it("should submit the explicitly selected model with the new assistant", async () => {
        const user = userEvent.setup();
        const add = vi.fn().mockReturnValue({ assistant: { id: 9 } });
        mockInvokeHandler("add_assistant", add);
        mockInvokeHandler("get_all_feature_config", () => [{ id: 1, feature_code: "butler_appearance", key: "butler_appearance_3", value: "{}" }]);
        const saveConfig = vi.fn();
        mockInvokeHandler("save_feature_config", saveConfig);
        render(<AddAssistantDialog assistantTypes={[{ code: 0, name: "普通" }, { code: 4, name: "Agent" }]} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await user.click(screen.getByRole("button", { name: "add" }));
        fireEvent.change(screen.getByLabelText("名称"), { target: { value: "New" } });
        await user.click(screen.getByRole("button", { name: "创建" }));
        expect(add).not.toHaveBeenCalled();
        await user.click(screen.getAllByRole("combobox")[1]);
        await user.click(screen.getByRole("option", { name: "Test model" }));
        await user.click(screen.getByRole("button", { name: "头部：贝雷帽" }));
        await user.click(screen.getByRole("button", { name: "创建" }));
        await waitFor(() => expect(add).toHaveBeenCalledWith({ name: "New", description: "", assistantType: 0, modelSelection: "test%%10" }));
        // 形象与编辑助手共用 butler_appearance 配置，保留其他助手已有的形象。
        await waitFor(() => expect(saveConfig).toHaveBeenCalledOnce());
        const config = saveConfig.mock.calls[0][0].config;
        expect(config.butler_appearance_3).toBe("{}");
        expect(JSON.parse(config.butler_appearance_9)).toMatchObject({ accessories: { head: "beret" }, motion: true });
    });
});
