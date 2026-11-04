import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import MobileArtifactPreview from "./MobileArtifactPreview";

vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => "android" }));
vi.mock("@/components/SaveArtifactDialog", () => ({
    default: ({ isOpen }: { isOpen: boolean }) => (isOpen ? <div role="dialog">保存对话框</div> : null),
}));

describe("MobileArtifactPreview component changes", () => {
    it("should not resend component source when an initialized frame loads again", async () => {
        render(<MobileArtifactPreview preview={{ language: "react", code: "export default () => null" }} />);
        const frame = await screen.findByTitle("隔离制品预览") as HTMLIFrameElement;
        const postMessage = vi.spyOn(frame.contentWindow!, "postMessage");
        fireEvent.load(frame);
        fireEvent.load(frame);
        expect(postMessage).not.toHaveBeenCalled();
        expect(screen.getByRole("alert")).toHaveTextContent("页面导航");
    });
    it("replaces the iframe for different code or language while keeping identical preview state", async () => {
        const first = { language: "react", code: "export default () => null", title: "example" };
        const { rerender } = render(<MobileArtifactPreview preview={first} />);
        const frame = await screen.findByTitle("隔离制品预览");
        fireEvent.click(screen.getByRole("button", { name: "放大" }));
        rerender(<MobileArtifactPreview preview={{ ...first }} />);
        expect(screen.getByTitle("隔离制品预览")).toBe(frame);
        expect(screen.getByText("125%")).toBeTruthy();
        rerender(<MobileArtifactPreview preview={{ ...first, code: "export default () => 1" }} />);
        await waitFor(() => expect(screen.getByTitle("隔离制品预览")).not.toBe(frame));
        const second = screen.getByTitle("隔离制品预览");
        rerender(<MobileArtifactPreview preview={{ ...first, language: "vue", code: "<template>Vue</template>" }} />);
        await waitFor(() => expect(screen.getByTitle("隔离制品预览")).not.toBe(second));
    });
    it("opens the save-to-collection dialog", async () => {
        render(<MobileArtifactPreview preview={{ language: "markdown", code: "# hi", title: "note" }} />);
        fireEvent.click(await screen.findByRole("button", { name: "保存到收藏" }));
        expect(await screen.findByRole("dialog")).toHaveTextContent("保存对话框");
    });
    it("does not offer re-saving a preview opened from collections", async () => {
        render(<MobileArtifactPreview preview={{ language: "markdown", code: "# hi", title: "note", collectionId: 3 }} />);
        expect(await screen.findByText("已收藏")).toBeTruthy();
        expect(screen.queryByRole("button", { name: "保存到收藏" })).toBeNull();
    });
});
