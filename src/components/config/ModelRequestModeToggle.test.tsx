import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ModelRequestModeToggle from "./ModelRequestModeToggle";

const openMenu = () => {
    const trigger = screen.getByRole("button", { name: /请求协议/ });
    fireEvent.keyDown(trigger, { key: "Enter" });
    return trigger;
};

describe("model protocol selector", () => {
    it("should retain the two OpenAI choices and change only on selection", async () => {
        const change = vi.fn();
        render(<ModelRequestModeToggle requestMode="chat_completions" onToggle={change} />);
        expect(screen.getByText("Chat")).toBeTruthy();
        openMenu();
        const items = await screen.findAllByRole("menuitemradio");
        expect(items).toHaveLength(2);
        expect(change).not.toHaveBeenCalled();
        fireEvent.click(screen.getByRole("menuitemradio", { name: "Responses" }));
        expect(change).toHaveBeenCalledWith("responses");
    });
    it("should mark the native Go protocol and select a translated protocol", async () => {
        const change = vi.fn();
        render(<ModelRequestModeToggle requestMode="anthropic_messages"
            availableModes={["anthropic_messages","responses"]} onToggle={change} />);
        expect(screen.getByText("Anthropic")).toBeTruthy();
        openMenu();
        expect(await screen.findByRole("menuitemradio", { name: "Anthropic Messages 推荐" })).toBeTruthy();
        expect(screen.getAllByRole("menuitemradio")).toHaveLength(2);
        fireEvent.click(screen.getByRole("menuitemradio", { name: "Responses" }));
        expect(change).toHaveBeenCalledWith("responses");
    });
    it("should show only the catalogued Go protocol or an unsupported label", () => {
        const { rerender } = render(<ModelRequestModeToggle requestMode="google_gemini"
            availableModes={["google_gemini"]} onToggle={vi.fn()} />);
        expect(screen.getByText("Gemini")).toBeTruthy();
        expect(screen.queryByRole("button")).toBeNull();
        rerender(<ModelRequestModeToggle requestMode="unsupported" availableModes={[]} onToggle={vi.fn()} />);
        expect(screen.getByText("协议待支持")).toBeTruthy();
    });
});
