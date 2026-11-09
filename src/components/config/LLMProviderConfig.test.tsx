import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "../../__tests__/mocks/tauri";
import LLMProviderConfig from "./LLMProviderConfig";

const providers = [
    { id: "1", name: "OpenAI", api_type: "openai_api", description: "", is_official: true, is_enabled: true },
];

async function openNewProviderDialog() {
    mockInvokeHandler("get_llm_providers", () => providers);
    mockInvokeHandler("get_platform", () => "windows");
    mockInvokeHandler("add_llm_provider", () => undefined);
    render(<LLMProviderConfig />);
    fireEvent.click(await screen.findByRole("button", { name: "添加提供商" }));
    return screen.findByLabelText("提供商名称");
}

describe("new provider dialog keyboard submit", () => {
    beforeEach(() => {
        clearAllMockHandlers();
        invoke.mockClear();
    });

    it("should keep the dialog and show an error when the name is empty", async () => {
        const input = await openNewProviderDialog();
        expect(input).toHaveAttribute("enterKeyHint", "done");
        fireEvent.click(screen.getByRole("button", { name: "确认" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("请填写提供商名称");
        expect(screen.getByRole("dialog")).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("add_llm_provider", expect.anything());
    });

    it("should not create a provider when the name duplicates an existing one", async () => {
        const input = await openNewProviderDialog();
        fireEvent.change(input, { target: { value: " OpenAI " } });
        fireEvent.click(screen.getByRole("button", { name: "确认" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("已有同名提供商");
        expect(invoke).not.toHaveBeenCalledWith("add_llm_provider", expect.anything());
    });

    it("should create a trimmed provider when the done key is pressed", async () => {
        const input = await openNewProviderDialog();
        fireEvent.change(input, { target: { value: "  我的 OpenAI  " } });
        fireEvent.keyDown(input, { key: "Enter" });
        await waitFor(() => {
            expect(invoke).toHaveBeenCalledWith("add_llm_provider", { name: "我的 OpenAI", apiType: "openai_api" });
        });
    });

    it("should not submit when Enter is still composing", async () => {
        const input = await openNewProviderDialog();
        fireEvent.change(input, { target: { value: "我的 OpenAI" } });
        fireEvent.keyDown(input, { key: "Enter", isComposing: true });
        expect(invoke).not.toHaveBeenCalledWith("add_llm_provider", expect.anything());
        expect(screen.getByRole("dialog")).toBeInTheDocument();
    });
});
