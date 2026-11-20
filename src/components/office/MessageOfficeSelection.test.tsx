import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: () => "" }));
const openModal = vi.fn();
vi.mock("@/utils/officeFile", async (importOriginal) => ({
    ...(await importOriginal<typeof import("@/utils/officeFile")>()),
    openOfficePreviewModal: (...args: unknown[]) => openModal(...args),
}));

import RawTextRenderer from "@/components/RawTextRenderer";
import { buildOfficeSelectionPrompt } from "@/utils/officeSelection";

const prompt = buildOfficeSelectionPrompt("把这句改正式", {
    refId: "ofr_0123456789abcdef",
    fileName: "季度报告.docx",
    format: "docx",
    selection: { format: "docx", selectedText: "这段话说得很随意", paragraphText: "这段话说得很随意，需要正式一些。" },
});

describe("MessageOfficeSelection", () => {
    beforeEach(() => openModal.mockReset());

    it("should show the file card in the bubble while keeping the selection body for the model only", () => {
        render(<RawTextRenderer content={prompt} />);
        expect(screen.getByText(/把这句改正式/)).toBeInTheDocument();
        const card = screen.getByTestId("message-office-selection");
        expect(card).toHaveTextContent("季度报告.docx");
        expect(card).toHaveTextContent("Word 正文中选中的文字");
        expect(card).toHaveTextContent("「这段话说得很随意」");
        expect(screen.queryByText(/所在段落全文/)).not.toBeInTheDocument();
        expect(screen.queryByText(/office_edit/)).not.toBeInTheDocument();
        expect(screen.queryByText(/<officeselection/i)).not.toBeInTheDocument();
    });

    it("should open the referenced file when the card is clicked", () => {
        render(<RawTextRenderer content={prompt} />);
        fireEvent.click(screen.getByTestId("message-office-selection"));
        expect(openModal).toHaveBeenCalledWith(
            expect.objectContaining({ title: "季度报告.docx", source: { refId: "ofr_0123456789abcdef" } }),
        );
    });
});
