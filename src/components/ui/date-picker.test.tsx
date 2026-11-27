import { fireEvent, render, screen, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { DatePicker, formatDateValue, parseDateValue } from "./date-picker";

describe("DatePicker", () => {
    it("should parse and format yyyy-MM-dd in local time without shifting the day", () => {
        const date = parseDateValue("2026-03-01");
        expect(date).toBeDefined();
        expect([date!.getFullYear(), date!.getMonth(), date!.getDate(), date!.getHours()]).toEqual([2026, 2, 1, 0]);
        expect(formatDateValue(date!)).toBe("2026-03-01");
        expect(formatDateValue(new Date(2026, 11, 31, 23, 59))).toBe("2026-12-31");
        expect(parseDateValue("")).toBeUndefined();
        expect(parseDateValue("2026-13-01")).toBeUndefined();
        expect(parseDateValue("2026/03/01")).toBeUndefined();
    });

    it("should show the selected date and report a picked day as yyyy-MM-dd", () => {
        const onChange = vi.fn();
        render(<DatePicker value="2026-03-01" onChange={onChange} aria-label="日期" />);
        const trigger = screen.getByRole("button", { name: "日期" });
        expect(trigger).toHaveTextContent("2026年3月1日");
        fireEvent.click(trigger);
        fireEvent.click(within(screen.getByRole("grid")).getByText("15"));
        expect(onChange).toHaveBeenCalledExactlyOnceWith("2026-03-15");
    });

    it("should show the placeholder when empty and clear a clearable value", () => {
        const onChange = vi.fn();
        const { rerender } = render(<DatePicker value="" onChange={onChange} placeholder="不限" clearable aria-label="截止日期" />);
        expect(screen.getByRole("button", { name: "截止日期" })).toHaveTextContent("不限");
        rerender(<DatePicker value="2026-03-01" onChange={onChange} placeholder="不限" clearable aria-label="截止日期" />);
        fireEvent.click(screen.getByRole("button", { name: "截止日期" }));
        fireEvent.click(screen.getByRole("button", { name: "清除" }));
        expect(onChange).toHaveBeenCalledExactlyOnceWith("");
    });
});
