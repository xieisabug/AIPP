import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { ColorPicker, hexToHsv, hsvToHex, normalizeHex } from "./color-picker";

describe("ColorPicker", () => {
    it("should normalize hex text and round-trip colors through HSV", () => {
        expect(normalizeHex("12ab34")).toBe("#12AB34");
        expect(normalizeHex(" #f0e0d0 ")).toBe("#F0E0D0");
        expect(normalizeHex("#12ab3")).toBeNull();
        expect(normalizeHex("#12ab3g")).toBeNull();
        for (const hex of ["#000000", "#FFFFFF", "#FF0000", "#12AB34", "#4D6BFE", "#D87855"]) {
            expect(hsvToHex(hexToHsv(hex))).toBe(hex);
        }
    });

    it("should report valid hex input in uppercase and ignore incomplete text", () => {
        const onChange = vi.fn();
        render(<ColorPicker value="#000000" onChange={onChange} label="左侧：自定义颜色" />);
        const input = screen.getByLabelText("左侧：自定义颜色：十六进制");
        fireEvent.change(input, { target: { value: "#12ab3" } });
        expect(onChange).not.toHaveBeenCalled();
        expect(input).toHaveAttribute("aria-invalid", "true");
        fireEvent.change(input, { target: { value: "#12ab34" } });
        expect(onChange).toHaveBeenCalledExactlyOnceWith("#12AB34");
    });

    it("should adjust hue from the keyboard", () => {
        const onChange = vi.fn();
        render(<ColorPicker value="#FF0000" onChange={onChange} label="颜色" />);
        fireEvent.keyDown(screen.getByRole("slider", { name: "颜色：色相" }), { key: "ArrowRight" });
        expect(onChange).toHaveBeenCalledExactlyOnceWith("#FF0400");
    });
});
