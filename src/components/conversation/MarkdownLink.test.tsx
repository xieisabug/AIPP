import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { openUrl } from "@tauri-apps/plugin-opener";
import { invoke } from "@tauri-apps/api/core";
import { OPEN_OFFICE_PREVIEW_EVENT } from "@/utils/officeFile";
import UnifiedMarkdown from "@/components/UnifiedMarkdown";
import { useMarkdownConfig } from "@/hooks/useMarkdownConfig";
import { MarkdownLink } from "./MarkdownLink";

vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn().mockResolvedValue(undefined) }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));
vi.mock("@/components/RustCodeBlock", () => ({ default: () => null }));
vi.mock("@/components/common/LazyImage", () => ({ default: () => null }));

beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue("preview-token");
});

function ChatMarkdown({ text }: { text: string }) {
    const config = useMarkdownConfig({ conversationId: 12 });
    return <UnifiedMarkdown markdownConfig={config}>{text}</UnifiedMarkdown>;
}

it.each([
    ["https://gitlab.example/team/app/-/pipelines/32", "workflow"],
    ["./app.json?raw=true#L32", "braces"],
    ["./main.rs#L200", "code-xml"],
    ["https://example.org/", "globe"],
    ["https://example.org/report.docx", "file-text"],
])("should show the target icon and open the original URL when clicked (%s)", (href, icon) => {
    render(<MarkdownLink href={href}>查看</MarkdownLink>);
    const link = screen.getByRole("link", { name: "查看" });
    expect(link.querySelector("svg")?.classList.contains(`lucide-${icon}`)).toBe(true);
    fireEvent.click(link);
    expect(openUrl).toHaveBeenCalledWith(href);
});

it.each([
    ["file:///C:/My%20Files/%E6%8A%A5%E5%91%8A.DOCX", "C:/My Files/报告.DOCX", "报告.DOCX", "docx"],
    ["C:\\docs\\book.xlsx", "C:/docs/book.xlsx", "book.xlsx", "xlsx"],
    ["/home/me/deck.pptx", "/home/me/deck.pptx", "deck.pptx", "pptx"],
    ["./files/%23draft%201.docx#section", "./files/#draft 1.docx", "#draft 1.docx", "docx"],
])("should preview the local file after Markdown sanitization when clicking %s", async (href, path, title, extension) => {
    const preview = vi.fn();
    window.addEventListener(OPEN_OFFICE_PREVIEW_EVENT, preview, { once: true });
    render(<ChatMarkdown text={`[查看](<${href}>)`} />);
    fireEvent.click(screen.getByRole("link", { name: "查看" }));
    await waitFor(() => expect(preview).toHaveBeenCalledOnce());
    expect(invoke).toHaveBeenCalledWith("handle_office_file_link", { path, conversationId: 12, action: "preview" });
    expect(preview.mock.calls[0][0].detail).toEqual({ title, extension, source: { relayToken: "preview-token" }, conversationId: 12 });
});

it("should use the external program when Ctrl-clicking a local Office link", () => {
    render(<ChatMarkdown text="[表格](./book.xlsx)" />);
    fireEvent.click(screen.getByRole("link"), { ctrlKey: true });
    expect(invoke).toHaveBeenCalledWith("handle_office_file_link", { path: "./book.xlsx", conversationId: 12, action: "open_externally" });
});

it.each([
    ["打开预览", "preview"],
    ["外部程序打开文件", "open_externally"],
    ["打开文件所在文件夹", "reveal"],
])("should perform the selected action when choosing %s from the file menu", async (name, action) => {
    render(<ChatMarkdown text="[幻灯片](./deck.pptx)" />);
    fireEvent.contextMenu(screen.getByRole("link"));
    expect(invoke).not.toHaveBeenCalledWith("handle_office_file_link", expect.anything());
    fireEvent.click(await screen.findByRole("menuitem", { name }));
    expect(invoke).toHaveBeenCalledWith("handle_office_file_link", { path: "./deck.pptx", conversationId: 12, action });
});

it("should preserve link sanitization when the file format or scheme is unsupported", () => {
    render(<ChatMarkdown text="[旧文件](file:///C:/old.doc) [其他协议](javascript:report.docx)" />);
    expect(screen.queryByRole("link")).toBeNull();
    expect(invoke).not.toHaveBeenCalledWith("handle_office_file_link", expect.anything());
});
