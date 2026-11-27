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
vi.mock("@/components/RustCodeBlock", () => ({ default: ({ children }: { children: string }) => <pre>{children}</pre> }));
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
    ["https://example.org/app.json?raw=true#L32", "braces"],
    ["https://example.org/main.rs#L200", "code-xml"],
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
    ["file:///C:/My%20Files/%E7%AC%94%E8%AE%B0.md", "C:/My Files/笔记.md", "markdown"],
    ["./src/main.rs#L12", "./src/main.rs", "rust"],
    ["main.rs:12", "main.rs", "rust"],
    ["C:\\work\\demo.ts:14:3", "C:/work/demo.ts", "typescript"],
    ["/home/me/%23draft.py:7", "/home/me/#draft.py", "python"],
])("opens preview_file content for a clicked local text link %s", async (href, path, language) => {
    const title = path.slice(path.lastIndexOf("/") + 1);
    const markdown = language === "markdown";
    const content = markdown ? "# 预览内容" : "const fence = '\x60\x60\x60';";
    vi.mocked(invoke).mockResolvedValue({
        request_id: "linked-file", files: [{ title, type: markdown ? "markdown" : "text", content, language }],
    });
    render(<ChatMarkdown text={`[查看](<${href}>)`} />);
    expect(invoke).not.toHaveBeenCalledWith("prepare_chat_text_file_preview", expect.anything());
    fireEvent.click(screen.getByRole("link", { name: "查看" }));
    expect(await screen.findByRole("dialog", { name: title })).toBeVisible();
    if (markdown) expect(await screen.findByRole("heading", { name: "预览内容" })).toBeVisible();
    else expect(await screen.findByText(content)).toBeVisible();
    expect(invoke).toHaveBeenCalledWith("prepare_chat_text_file_preview", { path, conversationId: 12, language });
    expect(openUrl).not.toHaveBeenCalled();
});

it("shows the file read error inside the preview dialog", async () => {
    vi.mocked(invoke).mockRejectedValue("无法定位文件 ./missing.md");
    render(<ChatMarkdown text="[查看](./missing.md)" />);
    fireEvent.click(screen.getByRole("link", { name: "查看" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("无法定位文件 ./missing.md");
});

it.each([
    ["report.CSV","file-spreadsheet","open_chat_preview_file"],
    ["sheet.tsv","file-spreadsheet","open_chat_preview_file"],
    ["photo.webp","image","open_chat_preview_file"],
    ["review.pdf","file-text","open_chat_preview_file"],
    ["clip.mp4","film","open_chat_preview_file"],
    ["track.mp3","music-2","open_chat_preview_file"],
    ["diagram.drawio.xml","workflow","prepare_chat_text_file_preview"],
    ["flow.mmd","workflow","prepare_chat_text_file_preview"],
    ["page.html","globe","prepare_chat_text_file_preview"],
    ["drawing.svg","image","prepare_chat_text_file_preview"],
    ["Dockerfile","code-xml","prepare_chat_text_file_preview"],
    ["release.patch","code-xml","prepare_chat_text_file_preview"],
])("chooses the icon and preview from the link target %s",async (name,icon,command)=>{
    vi.mocked(invoke).mockRejectedValue("文件不可读");
    render(<ChatMarkdown text={`[查看](file:///C:/docs/${name})`} />);
    const link=screen.getByRole("link",{name:"查看"});
    expect(link.querySelector("svg")).toHaveClass(`lucide-${icon}`);
    fireEvent.click(link);
    expect(await screen.findByRole("dialog",{name})).toBeVisible();
    expect(await screen.findByRole("alert")).toHaveTextContent("文件不可读");
    expect(invoke).toHaveBeenCalledWith(command,expect.objectContaining({path:"C:/docs/"+name,conversationId:12}));
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

it("reveals an unsupported local file and preserves sanitization of other schemes", () => {
    render(<ChatMarkdown text="[旧文件](file:///C:/old.doc) [其他协议](javascript:report.docx) [源码](javascript:main.rs) [网络共享](file://server/docs/notes.md)" />);
    expect(screen.getAllByRole("link")).toHaveLength(1);
    fireEvent.click(screen.getByRole("link", {name:"旧文件"}));
    expect(invoke).toHaveBeenCalledWith("reveal_chat_file",{path:"C:/old.doc",conversationId:12});
    expect(invoke).not.toHaveBeenCalledWith("handle_office_file_link", expect.anything());
});
