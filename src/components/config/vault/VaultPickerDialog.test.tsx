import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import VaultPickerDialog from "./VaultPickerDialog";

vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));

const ID = "11111111-2222-4333-8444-555555555555";

describe("VaultPickerDialog exact-value migration", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        vi.mocked(invoke).mockClear();
        mockInvokeHandler("vault_create", () => ({ id: ID }));
    });

    it.each([
        ["  fake password  ", "", "  fake password  "],
        ["\t fake token\t", "", "\t fake token\t"],
        ["\r\nfirst line\nSECOND=value\r\n", "", "\r\nfirst line\nSECOND=value\r\n"],
        ["Bearer fake-token  ", "Bearer ", "fake-token  "],
        ["bEaReR \t fake-token\t", "bEaReR \t ", "fake-token\t"],
    ])("should preserve the original bytes when storing %j", async (currentValue, prefix, value) => {
        const user = userEvent.setup();
        const onPick = vi.fn();
        const { container } = render(<VaultPickerDialog open items={[]} currentValue={currentValue}
            suggestedName="Credential" onClose={vi.fn()} onPick={onPick} />);
        expect(container.textContent).not.toContain("fake-token");
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_create", {
            input: expect.objectContaining({ value }),
        }));
        await waitFor(() => expect(onPick).toHaveBeenCalledWith({
            ref: { id: ID }, prefix, variableName: undefined, storedCurrentValue: true,
        }));
        expect(prefix + value).toBe(currentValue);
    });

    it("should not offer storing a whitespace-only value", () => {
        render(<VaultPickerDialog open items={[]} currentValue={" \t\r\n "}
            suggestedName="Credential" onClose={vi.fn()} onPick={vi.fn()} />);
        expect(screen.queryByRole("radio", { name: "把当前填写的值存入保险箱" })).not.toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalled();
    });
});
