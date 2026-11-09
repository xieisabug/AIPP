import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import ConversationExportDialog from "./ConversationExportDialog";

const getExportData = vi.hoisted(() => vi.fn());

vi.mock("@/services/conversationExportService", () => ({
    conversationExportService: { getExportData },
}));
vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => "android" }));

describe("ConversationExportDialog load failure (#162)", () => {
    beforeEach(() => {
        getExportData.mockReset();
    });

    it("shows a readable error with retry instead of dead export buttons", async () => {
        getExportData.mockRejectedValueOnce(new Error("数据库已锁定"));
        getExportData.mockResolvedValueOnce({
            conversation: { conversation: { name: "会话" }, messages: [] },
            toolCalls: [],
        });

        render(<ConversationExportDialog conversationId="7" externalOpen onExternalOpenChange={vi.fn()} hideTrigger />);

        expect(await screen.findByText("读取会话内容失败，暂时无法导出")).toBeInTheDocument();
        expect(screen.getByText(/数据库已锁定/)).toBeInTheDocument();
        expect(screen.queryByText("Markdown")).not.toBeInTheDocument();

        fireEvent.click(screen.getByRole("button", { name: /重试/ }));
        await waitFor(() => expect(getExportData).toHaveBeenCalledTimes(2));
        expect(await screen.findByText("Markdown")).toBeInTheDocument();
        expect(screen.queryByText("读取会话内容失败，暂时无法导出")).not.toBeInTheDocument();
        expect(screen.getByText(/PDF 通过系统打印生成/)).toBeInTheDocument();
    });
});
