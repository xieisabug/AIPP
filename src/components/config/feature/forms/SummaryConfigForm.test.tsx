import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReviewOutputCapability } from "./SummaryConfigForm";
import { clearAllMockHandlers, invoke, mockInvokeHandler } from "@/__tests__/mocks/tauri";

describe("ReviewOutputCapability", () => {
    afterEach(() => { clearAllMockHandlers(); vi.clearAllMocks(); });

    it("should reset the selected endpoint memory when recheck is clicked", async () => {
        let prompt = true;
        mockInvokeHandler("get_tool_review_output_capability", () => ({ prompt, reason: "unsupported" }));
        mockInvokeHandler("reset_tool_review_output_capability", () => { prompt = false; });
        render(<ReviewOutputCapability model="deepseek-v4-flash%%41" mode="auto" />);
        expect(await screen.findByText(/当前使用 Prompt/)).toBeInTheDocument();
        await userEvent.setup().click(screen.getByRole("button", { name: "重新检测" }));
        expect(await screen.findByText(/下次审核优先尝试 JSON Schema/)).toBeInTheDocument();
        expect(invoke).toHaveBeenCalledWith("reset_tool_review_output_capability", {
            providerId: 41, modelCode: "deepseek-v4-flash",
        });
    });

    it("should show a diagnostic when capability lookup fails", async () => {
        mockInvokeHandler("get_tool_review_output_capability", () => { throw new Error("model missing"); });
        render(<ReviewOutputCapability model="review%%41" mode="auto" />);
        expect(await screen.findByText(/读取检测结果失败.*model missing/)).toBeInTheDocument();
    });

    it("should avoid capability lookup when an explicit output mode is selected", async () => {
        render(<ReviewOutputCapability model="review%%41" mode="schema" />);
        await waitFor(() => expect(invoke).not.toHaveBeenCalledWith("get_tool_review_output_capability", expect.anything()));
        expect(screen.queryByRole("button", { name: "重新检测" })).not.toBeInTheDocument();
    });
});
