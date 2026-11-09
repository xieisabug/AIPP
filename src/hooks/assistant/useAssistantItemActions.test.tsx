import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { mockInvokeHandler, clearAllMockHandlers } from "@/__tests__/mocks/tauri";
import { useAssistantItemActions } from "./useAssistantItemActions";

vi.mock("@/components/config/EditAssistantDialog", () => ({ default: () => null }));
const assistant = { id: 8, name: "目标助手", assistant_type: 0 };
function Harness({ onClear = vi.fn() }: { onClear?: () => Promise<void> }) {
    const actions = useAssistantItemActions({ onClear });
    return <>{actions.dialogs}
        <button onClick={() => void actions.run(assistant, "delete")}>delete target</button>
        <button onClick={() => void actions.run(assistant, "clear")}>clear target</button>
        <button onClick={() => void actions.run(assistant, "copy")}>copy target</button>
    </>;
}
describe("useAssistantItemActions", () => {
    afterEach(clearAllMockHandlers);
    it("should delete the requested assistant only after confirmation", async () => {
        const remove = vi.fn();
        mockInvokeHandler("delete_assistant", remove);
        render(<Harness />);
        fireEvent.click(screen.getByText("delete target"));
        expect(remove).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "取消" }));
        expect(remove).not.toHaveBeenCalled();
        fireEvent.click(screen.getByText("delete target"));
        fireEvent.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(remove).toHaveBeenCalledWith({ assistantId: 8 }));
    });
    it("should confirm history clearing separately from deleting the assistant", async () => {
        const clear = vi.fn().mockResolvedValue(undefined);
        const remove = vi.fn();
        mockInvokeHandler("delete_assistant", remove);
        render(<Harness onClear={clear} />);
        fireEvent.click(screen.getByText("clear target"));
        expect(clear).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(clear).toHaveBeenCalledWith(assistant));
        expect(remove).not.toHaveBeenCalled();
    });
    it("should copy using the requested assistant ID", async () => {
        const copy = vi.fn().mockReturnValue({ assistant: { id: 9 } });
        mockInvokeHandler("copy_assistant", copy);
        render(<Harness />);
        fireEvent.click(screen.getByText("copy target"));
        await waitFor(() => expect(copy).toHaveBeenCalledWith({ assistantId: 8 }));
    });
});
