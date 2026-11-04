import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ModelRequestModeToggle from "./ModelRequestModeToggle";

describe("model protocol selector", () => {
    it("should retain the two OpenAI choices and change only on selection", () => {
        const change = vi.fn();
        render(<ModelRequestModeToggle requestMode="chat_completions" onToggle={change} />);
        expect(screen.getAllByRole("option")).toHaveLength(2);
        fireEvent.click(screen.getByRole("combobox"));
        expect(change).not.toHaveBeenCalled();
        fireEvent.change(screen.getByRole("combobox"), { target: { value: "responses" } });
        expect(change).toHaveBeenCalledWith("responses");
    });
    it("should mark the native Go protocol and select a translated protocol", () => {
        const change = vi.fn();
        render(<ModelRequestModeToggle requestMode="anthropic_messages"
            availableModes={["anthropic_messages","responses"]} onToggle={change} />);
        expect(screen.getByRole("option", { name: "Anthropic Messages（推荐）" })).toBeTruthy();
        expect(screen.getAllByRole("option")).toHaveLength(2);
        fireEvent.change(screen.getByRole("combobox"), { target: { value: "responses" } });
        expect(change).toHaveBeenCalledWith("responses");
    });
    it("should show only the catalogued Go protocol or an unsupported label", () => {
        const { rerender } = render(<ModelRequestModeToggle requestMode="google_gemini"
            availableModes={["google_gemini"]} onToggle={vi.fn()} />);
        expect(screen.getByText("Google Gemini")).toBeTruthy();
        expect(screen.queryByRole("combobox")).toBeNull();
        rerender(<ModelRequestModeToggle requestMode="unsupported" availableModes={[]} onToggle={vi.fn()} />);
        expect(screen.getByText("协议待支持")).toBeTruthy();
    });
});
