import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { TimePicker, formatTimeValue, parseTimeValue } from "./time-picker";

describe("TimePicker", () => {
    it("should parse and format HH:mm", () => {
        expect(parseTimeValue("09:05")).toEqual({ hour: 9, minute: 5 });
        expect(parseTimeValue("9:05")).toEqual({ hour: 9, minute: 5 });
        expect(parseTimeValue("24:00")).toBeNull();
        expect(parseTimeValue("12:60")).toBeNull();
        expect(parseTimeValue("")).toBeNull();
        expect(formatTimeValue(7, 3)).toBe("07:03");
    });

    it("should show the value and report picked hour and minute as HH:mm", () => {
        const onChange = vi.fn();
        const { rerender } = render(<TimePicker value="22:30" onChange={onChange} aria-label="时刻" />);
        const trigger = screen.getByRole("button", { name: "时刻" });
        expect(trigger).toHaveTextContent("22:30");
        fireEvent.click(trigger);
        const hours = screen.getByRole("listbox", { name: "小时" });
        expect(within(hours).getByRole("option", { name: "22" })).toHaveAttribute("aria-selected", "true");
        fireEvent.click(within(hours).getByRole("option", { name: "08" }));
        expect(onChange).toHaveBeenLastCalledWith("08:30");
        rerender(<TimePicker value="08:30" onChange={onChange} aria-label="时刻" />);
        fireEvent.click(within(screen.getByRole("listbox", { name: "分钟" })).getByRole("option", { name: "07" }));
        expect(onChange).toHaveBeenLastCalledWith("08:07");
    });

    it("should keep an off-step minute selectable and show the placeholder when empty", () => {
        const { rerender } = render(<TimePicker value="" onChange={vi.fn()} aria-label="时刻" />);
        expect(screen.getByRole("button", { name: "时刻" })).toHaveTextContent("--:--");
        rerender(<TimePicker value="10:07" onChange={vi.fn()} minuteStep={15} aria-label="时刻" />);
        fireEvent.click(screen.getByRole("button", { name: "时刻" }));
        const minutes = within(screen.getByRole("listbox", { name: "分钟" })).getAllByRole("option").map(option => option.textContent);
        expect(minutes).toEqual(["00", "07", "15", "30", "45"]);
    });
});
