import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import MessageEditDialog from "./MessageEditDialog";

vi.mock("../hooks/use-mobile", () => ({ useIsMobile: () => false }));
vi.mock("../hooks/use-platform", () => ({ useIsMobilePlatform: () => true }));

describe("MessageEditDialog mobile", () => {
    it("uses a full screen panel and saves edited user content with regeneration", () => {
        const onClose = vi.fn();
        const onSaveAndRegenerate = vi.fn();
        render(<MessageEditDialog isOpen initialContent="before" messageType="user" onClose={onClose} onSave={vi.fn()} onSaveAndRegenerate={onSaveAndRegenerate} />);
        const dialog = screen.getByRole("dialog", { name: "编辑消息" });
        expect(dialog).toHaveAttribute("data-mobile-panel");
        expect(dialog.className).toContain("fixed inset-0");
        fireEvent.change(screen.getByPlaceholderText("请输入消息内容..."), { target: { value: "after" } });
        fireEvent.click(screen.getByRole("button", { name: "修改并重新生成" }));
        expect(onSaveAndRegenerate).toHaveBeenCalledWith("after");
        expect(onClose).toHaveBeenCalledOnce();
    });

    it("closes on Escape and does not offer regeneration for a response", () => {
        const onClose = vi.fn();
        render(<MessageEditDialog isOpen initialContent="response" messageType="response" onClose={onClose} onSave={vi.fn()} onSaveAndRegenerate={vi.fn()} />);
        expect(screen.queryByRole("button", { name: "修改并重新生成" })).toBeNull();
        fireEvent.keyDown(screen.getByRole("dialog"), { key: "Escape" });
        expect(onClose).toHaveBeenCalledOnce();
    });
});
