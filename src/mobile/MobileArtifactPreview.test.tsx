import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import MobileArtifactPreview from "./MobileArtifactPreview";

vi.mock("@/hooks/use-platform", () => ({ usePlatform: () => "android", useIsMobilePlatform: () => true }));
vi.mock("@/components/SaveArtifactDialog", () => ({
    default: ({ isOpen }: { isOpen: boolean }) => (isOpen ? <div role="dialog">保存对话框</div> : null),
}));

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

describe("MobileArtifactPreview component changes", () => {
    it("should not resend component source when an initialized frame loads again", async () => {
        render(<MobileArtifactPreview preview={{ language: "react", code: "export default () => null" }} />);
        const frame = await screen.findByTitle("隔离 Artifact 预览") as HTMLIFrameElement;
        const postMessage = vi.spyOn(frame.contentWindow!, "postMessage");
        fireEvent.load(frame);
        fireEvent.load(frame);
        expect(postMessage).not.toHaveBeenCalled();
        expect(screen.getByRole("alert")).toHaveTextContent("页面导航");
    });
    it("replaces the iframe for different code or language while keeping identical preview state", async () => {
        const first = { language: "react", code: "export default () => null", title: "example" };
        const { rerender } = render(<MobileArtifactPreview preview={first} />);
        const frame = await screen.findByTitle("隔离 Artifact 预览");
        fireEvent.click(screen.getByRole("button", { name: "放大" }));
        rerender(<MobileArtifactPreview preview={{ ...first }} />);
        expect(screen.getByTitle("隔离 Artifact 预览")).toBe(frame);
        expect(screen.getByText("125%")).toBeTruthy();
        rerender(<MobileArtifactPreview preview={{ ...first, code: "export default () => 1" }} />);
        await waitFor(() => expect(screen.getByTitle("隔离 Artifact 预览")).not.toBe(frame));
        const second = screen.getByTitle("隔离 Artifact 预览");
        rerender(<MobileArtifactPreview preview={{ ...first, language: "vue", code: "<template>Vue</template>" }} />);
        await waitFor(() => expect(screen.getByTitle("隔离 Artifact 预览")).not.toBe(second));
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

it("should preview a published Markdown project and browse its snapshot on mobile", async () => {
    const ref = { conversationId: 32, requestId: "saved", candidateId: "book" };
    const code = "AIPP_CHOICE_PREVIEW_V1\n" + JSON.stringify(ref);
    const content = "# Book\n![Cover][cover]\n\n[cover]: ../img/front%20cover.png";
    const files = { "docs/index.md": btoa(content), "img/front cover.png": "AA==" };
    invoke.mockImplementation(async (name: string) => {
        if (name === "get_present_choices") return { request: { candidates: [
            { id: "book", title: "Book", type: "artifact", entry_file: "docs/index.md", content }
        ] }};
        if (name === "get_choice_project") return files;
        throw Error("unexpected " + name);
    });
    const preview = { language: "html", code, conversationId: 32 };
    const { rerender } = render(<MobileArtifactPreview preview={preview} />);
    const frame = await screen.findByTitle("Book");
    // 只放行带 nonce 的测高脚本，不给 allow-same-origin。
    expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    expect(frame.getAttribute("srcdoc")).toMatch(/script-src 'nonce-[0-9a-f]{32}'/);
    expect(frame.getAttribute("srcdoc")).toContain('src="data:image/png;base64,AA=="');
    fireEvent.click(screen.getByRole("radio", { name: "工程源码" }));
    expect(screen.getByRole("combobox")).toHaveValue("docs/index.md");
    fireEvent.click(screen.getByRole("radio", { name: "预览" }));
    expect(await screen.findByTitle("Book")).toBeInTheDocument();
    expect(invoke).not.toHaveBeenCalledWith("open_choice_build", expect.anything());
    invoke.mockClear();
    rerender(<MobileArtifactPreview preview={{ ...preview, conversationId: 33 }} />);
    expect(await screen.findByRole("alert")).toHaveTextContent("其他会话");
    expect(invoke).not.toHaveBeenCalled();
});

it("should reopen a collected project using only its collection ID", async () => {
    invoke.mockClear();
    invoke.mockImplementation(async (name: string) => {
        if (name !== "get_collected_choice") throw Error(name);
        return {candidate: {type: "artifact", id: "book", entry_file: "book.md", title: "Book", content: "![Cover](pic.svg)"},
            files: {"book.md": btoa("![Cover](pic.svg)"), "pic.svg": btoa("<svg/>")}};
    });
    render(<MobileArtifactPreview preview={{language: "html", code: "AIPP_CHOICE_ARCHIVE_V1\n", collectionId: 7}} />);
    const frame = await screen.findByTitle("Book");
    expect(frame.getAttribute("srcdoc")).toContain("data:image/svg+xml;base64,");
    expect(invoke.mock.calls).toEqual([["get_collected_choice", {collectionId: 7}]]);
});
