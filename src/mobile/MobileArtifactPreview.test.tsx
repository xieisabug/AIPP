import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import MobileArtifactPreview from "./MobileArtifactPreview";

vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => "android" }));

describe("MobileArtifactPreview component changes", () => {
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
});
