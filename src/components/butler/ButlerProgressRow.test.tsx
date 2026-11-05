import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { afterEach, describe, expect, it } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerProgressRow from "./ButlerProgressRow";
import { BUTLER_PROGRESS_EVENT } from "./butlerProgress";

const summary = (conversation_id: number, epoch: number, text: string) => ({
    conversation_id, epoch, text, steps: [{ label: "web_search 竞品定价", status: "success" }, { label: "read_file 报价.xlsx", status: "executing" }],
});

describe("ButlerProgressRow", () => {
    afterEach(() => clearAllMockHandlers());

    it("keeps the plain processing text without a helper summary", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => null);
        render(<ButlerProgressRow conversationId={3} />);
        expect(await screen.findByText("正在处理…")).toBeTruthy();
    });

    it("shows the latest summary for its conversation and expands the steps", async () => {
        mockInvokeHandler("get_butler_progress_summary", () => null);
        render(<ButlerProgressRow conversationId={3} />);
        await act(async () => {
            await emit(BUTLER_PROGRESS_EVENT, summary(4, 1, "别的会话"));
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 2, "正在对比三家报价"));
            await emit(BUTLER_PROGRESS_EVENT, summary(3, 1, "上一轮遗留"));
        });
        await waitFor(() => expect(screen.getByText("正在对比三家报价")).toBeTruthy());
        expect(screen.queryByText("上一轮遗留")).toBeNull();
        fireEvent.click(screen.getByRole("button"));
        expect(screen.getByText("read_file 报价.xlsx")).toBeTruthy();
    });
});
