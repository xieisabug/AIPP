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
        render(<AddAssistantDialog assistantTypes={[{ code: 0, name: "普通" }, { code: 4, name: "Agent" }]} onAssistantAdded={vi.fn()} triggerButtonProps={{ "aria-label": "add" }} />);
        await user.click(screen.getByRole("button", { name: "add" }));
        fireEvent.change(screen.getByLabelText("名称"), { target: { value: "New" } });
        await user.click(screen.getByRole("button", { name: "确认" }));
        expect(add).not.toHaveBeenCalled();
        await user.click(screen.getAllByRole("combobox")[1]);
        await user.click(screen.getByRole("option", { name: "Test model" }));
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(add).toHaveBeenCalledWith({ name: "New", description: "", assistantType: 0, modelSelection: "test%%10" }));
    });
});
