import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { ChoicePreview, CollectedChoicePreview, SavedChoicePreview } from "./ChoicePreview";
import type { Candidate } from "@/utils/presentChoices";

const { invoke, convertFileSrc } = vi.hoisted(() => ({
    invoke: vi.fn(),
    convertFileSrc: vi.fn((path: string, protocol = "asset") => `${protocol}://localhost/${path}`),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));

beforeEach(() => {
    invoke.mockReset();
    convertFileSrc.mockClear();
});

it("should keep a page-sized loading placeholder through prepare, document creation, and iframe load", async () => {
    let resolvePrepare!: (value: unknown) => void;
    let resolveDoc!: (value: string) => void;
    invoke.mockImplementation((name: string) => {
        if (name === "prepare_preview_code_request_for_ui") {
            return new Promise((resolve) => {
                resolvePrepare = resolve;
            });
        }
        if (name === "create_choice_document") {
            return new Promise<string>((resolve) => {
                resolveDoc = resolve;
            });
        }
        return Promise.resolve(undefined);
    });

    const candidate: Candidate = {
        type: "html",
        id: "cand-1",
        title: "方案一",
        content: "<html><body><h1>Hello</h1></body></html>",
    };

    render(
        <ChoicePreview
            candidate={candidate}
            reference={{ conversationId: 1, requestId: "req-1", candidateId: "cand-1" }}
        />
    );

    const preparingStatus = screen.getByRole("status", { name: "正在准备预览…" });
    expect(preparingStatus).toBeInTheDocument();
    expect(preparingStatus.className).toContain("w-full");
    expect(preparingStatus.className).toContain("h-[min(75vh,max(24rem,70vh))]");

    resolvePrepare({
        title: "方案一",
        renderer: "html",
        code: candidate.content,
    });

    await waitFor(() => expect(invoke).toHaveBeenCalledWith("create_choice_document", expect.any(Object)));
    expect(screen.getByRole("status", { name: "正在准备预览…" })).toBeInTheDocument();

    resolveDoc("token-123");

    const frame = await screen.findByTitle("方案一");
    expect(frame.className).toContain("h-[min(75vh,max(24rem,70vh))]");
    expect(frame.className).toContain("opacity-0");
    expect(screen.getByRole("status", { name: "正在准备预览…" })).toBeInTheDocument();

    fireEvent.load(frame);
    expect(screen.queryByRole("status", { name: "正在准备预览…" })).toBeNull();
    expect(frame.className).not.toContain("opacity-0");
});

it("should render page-sized loading placeholders for saved, collected, and project previews", () => {
    invoke.mockImplementation(() => new Promise(() => {}));

    const { unmount } = render(
        <SavedChoicePreview reference={{ conversationId: 1, requestId: "req-1", candidateId: "cand-1" }} />
    );
    expect(screen.getByRole("status", { name: "正在读取快照…" }).className).toContain("h-[min(75vh,max(24rem,70vh))]");
    unmount();

    const collected = render(<CollectedChoicePreview collectionId={9} />);
    expect(screen.getByRole("status", { name: "正在读取收藏快照…" }).className).toContain("h-[min(75vh,max(24rem,70vh))]");
    collected.unmount();

    render(
        <ChoicePreview
            candidate={{ type: "artifact", id: "art-1", title: "工程一", content: "", entry_file: "index.html" }}
            reference={{ conversationId: 1, requestId: "req-1", candidateId: "art-1" }}
        />
    );
    expect(screen.getByRole("status", { name: "正在读取工程…" }).className).toContain("h-[min(75vh,max(24rem,70vh))]");
});
