import { fireEvent, render, screen } from "@testing-library/react";
import { expect, it, vi } from "vitest";
import ModelSelectionDialog from "./ModelSelectionDialog";

it("should show model names and submit recommended protocols for new selections", () => {
    const onConfirm = vi.fn();
    const onOpenChange = vi.fn();
    const common = { description: "", vision_support: true, audio_support: false, video_support: false,
        available_modes: ["anthropic_messages", "responses"], request_mode: "responses" };
    render(<ModelSelectionDialog open loading={false} onConfirm={onConfirm} onOpenChange={onOpenChange}
        modelData={{ missing_models: [], available_models: [
            { ...common, name: "MiniMax M3", code: "minimax-m3", is_selected: false },
            { ...common, name: "Existing", code: "existing", is_selected: true },
        ] }} />);
    expect(screen.getByText("MiniMax M3")).toBeVisible();
    expect(screen.queryByRole("combobox")).toBeNull();
    fireEvent.click(screen.getByRole("checkbox", { name: "MiniMax M3" }));
    fireEvent.click(screen.getByRole("button", { name: "确认更新" }));
    expect(onConfirm).toHaveBeenCalledWith([
        expect.objectContaining({ code: "minimax-m3", is_selected: true, request_mode: "anthropic_messages" }),
        expect.objectContaining({ code: "existing", is_selected: true, request_mode: "responses" }),
    ]);
    expect(onOpenChange).not.toHaveBeenCalled();
});
