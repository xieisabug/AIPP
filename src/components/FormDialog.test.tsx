import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import FormDialog from "./FormDialog";

describe("FormDialog mobile keyboard chrome", () => {
    it("keeps confirm/cancel actions present for the visual viewport footer", () => {
        render(
            <FormDialog title="新增提供商" isOpen onClose={vi.fn()} onSubmit={vi.fn()}>
                <input aria-label="名称" />
            </FormDialog>,
        );
        expect(screen.getByText("确认")).toBeTruthy();
        expect(screen.getByText("取消")).toBeTruthy();
        expect(document.querySelector('[data-aipp-slot="form-dialog-confirm"]')).toBeTruthy();
        expect(document.querySelector('[data-aipp-slot="form-dialog"]')?.className).toMatch(/aipp-vv-height/);
    });
});
