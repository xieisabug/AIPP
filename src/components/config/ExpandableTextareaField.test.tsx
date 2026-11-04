import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ExpandableTextareaField from "./ExpandableTextareaField";

const platform = vi.hoisted(() => ({ mobile: true }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => platform.mobile }));

describe("ExpandableTextareaField platform layout", () => {
    it("uses a full screen editor and large trigger on mobile", () => {
        platform.mobile = true;
        render(<ExpandableTextareaField label="提示词" fieldRenderData={{ value: "draft", onChange: vi.fn() }} />);
        const trigger = screen.getByRole("button", { name: "扩展编辑" });
        expect(trigger).toHaveClass("h-11", "w-11");
        fireEvent.click(trigger);
        const dialog = screen.getByRole("dialog");
        expect(dialog).toHaveClass("inset-0", "h-screen", "safe-top", "safe-bottom");
        expect(screen.getAllByDisplayValue("draft")).toHaveLength(2);
    });
    it("retains desktop dialog sizing", () => {
        platform.mobile = false;
        render(<ExpandableTextareaField label="提示词" fieldRenderData={{ value: "draft", onChange: vi.fn() }} />);
        fireEvent.click(screen.getByRole("button", { name: "扩展编辑" }));
        expect(screen.getByRole("dialog")).toHaveClass("w-[70vw]", "h-[80vh]");
    });
});
