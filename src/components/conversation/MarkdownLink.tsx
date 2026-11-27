import { lazy, Suspense, useState, type ComponentProps } from "react";
import type { ExtraProps } from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
    Braces,
    CodeXml,
    File,
    FileSpreadsheet,
    FileText,
    Film,
    FolderGit2,
    Globe,
    Hash,
    Image,
    Mail,
    Music2,
    Presentation,
    Workflow,
} from "lucide-react";
import { toast } from "sonner";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { parseLocalFileLink, type LocalFileLink } from "@/react-markdown/localFileLink";
import { openOfficePreviewModal } from "@/utils/officeFile";
import { CompactDialogHeader, Dialog, DialogContent, DialogTrigger } from "@/components/ui/dialog";

const ChatTextFilePreview = lazy(() => import("./ChatTextFilePreview"));
const ChatBinaryFilePreview = lazy(() => import("../file-preview/ChatBinaryFilePreview"));

function iconForFile(file: LocalFileLink) {
    switch (file.kind) {
        case "office":
            return file.extension === "xlsx" ? FileSpreadsheet : file.extension === "pptx" ? Presentation : FileText;
        case "table":
            return FileSpreadsheet;
        case "image":
            return Image;
        case "pdf":
            return FileText;
        case "audio":
            return Music2;
        case "video":
            return Film;
        case "artifact":
            return file.language === "svg" ? Image : file.language === "html" ? Globe : Workflow;
        case "text":
            return ["markdown", "text"].includes(file.language)
                ? FileText
                : ["json", "yaml", "toml", "ini"].includes(file.language)
                ? Braces
                : CodeXml;
        case "reveal":
            return File;
    }
}

function iconForLink(href: string) {
    if (href.startsWith("#")) return Hash;
    let url: URL;
    try {
        url = new URL(href, "https://relative.invalid");
    } catch {
        return Globe;
    }
    if (url.protocol === "mailto:") return Mail;
    const path = url.pathname.toLowerCase();
    if (/\/-\/(?:pipelines|jobs)(?:\/|$)|\/actions\/runs\//.test(path)) return Workflow;
    if (/\.(?:jsonc?|toml|ya?ml)$/.test(path)) return Braces;
    if (/\.(?:rs|[cm]?[jt]sx?|py|go|java|cs|cpp|c|h|html|css|sh|ps1)$/.test(path)) return CodeXml;
    if (/\.(?:md|txt|pdf|docx?)$/.test(path)) return FileText;
    if (/\.(?:png|jpe?g|gif|webp|svg)$/.test(path)) return Image;
    if (/(^|\.)(github\.com|gitlab\.com)$/.test(url.hostname) || path.includes("/-/")) return FolderGit2;
    return Globe;
}

type MarkdownLinkProps = ComponentProps<"a"> & ExtraProps & { conversationId?: number; enableLocalFileLinks?: boolean };

export function MarkdownLink({
    node: _node,
    href,
    children,
    className,
    conversationId,
    enableLocalFileLinks = true,
    ...props
}: MarkdownLinkProps) {
    const localFile = enableLocalFileLinks ? parseLocalFileLink(href) : null;
    const office = localFile?.kind === "office" ? localFile : null;
    const [previewOpen, setPreviewOpen] = useState(false);
    const Icon = localFile ? iconForFile(localFile) : href ? iconForLink(href) : null;
    const handleOfficeAction = async (action: "preview" | "open_externally" | "reveal") => {
        if (!office) return;
        try {
            const relayToken = await invoke<string | null>("handle_office_file_link", {
                path: office.path,
                conversationId,
                action,
            });
            if (action === "preview") {
                if (!relayToken) throw new Error("未取得文件预览凭据");
                openOfficePreviewModal({
                    title: office.title,
                    extension: office.extension,
                    source: { relayToken },
                    conversationId,
                });
            }
        } catch (error) {
            toast.error(String(error));
        }
    };
    const link = (
        <a
            {...props}
            href={href}
            className={`aipp-markdown-link ${className ?? ""}`}
            onClick={(event) => {
                event.preventDefault();
                if (office) void handleOfficeAction(event.ctrlKey || event.metaKey ? "open_externally" : "preview");
                else if (localFile?.kind === "reveal")
                    void invoke("reveal_chat_file", { path: localFile.path, conversationId }).catch((e) =>
                        toast.error(String(e))
                    );
                else if (localFile) setPreviewOpen(true);
                else if (href) openUrl(href).catch(console.error);
            }}
        >
            {Icon && <Icon aria-hidden="true" className="me-[0.25em] inline-block size-[0.9em] align-[-0.1em]" />}
            {children}
        </a>
    );
    if (localFile && localFile.kind !== "office" && localFile.kind !== "reveal") {
        return (
            <Dialog open={previewOpen} onOpenChange={setPreviewOpen}>
                <DialogTrigger asChild>{link}</DialogTrigger>
                {previewOpen && (
                    <DialogContent
                        aria-describedby={undefined}
                        showCloseButton={false}
                        className="flex h-[90dvh] w-[94vw] max-w-none flex-col gap-0 overflow-hidden p-0 sm:max-w-5xl"
                    >
                        <CompactDialogHeader
                            title={localFile.title}
                            icon={Icon && <Icon className="size-4 text-muted-foreground" />}
                        />
                        <div className="flex min-h-0 min-w-0 flex-1 flex-col overflow-auto">
                            <Suspense fallback={<p>正在读取文件…</p>}>
                                {localFile.kind === "text" || localFile.kind === "artifact" ? (
                                    <ChatTextFilePreview
                                        path={localFile.path}
                                        language={localFile.language}
                                        conversationId={conversationId}
                                    />
                                ) : (
                                    <ChatBinaryFilePreview file={localFile} conversationId={conversationId} />
                                )}
                            </Suspense>
                        </div>
                    </DialogContent>
                )}
            </Dialog>
        );
    }
    if (!office) return link;
    return (
        <ContextMenu>
            <ContextMenuTrigger asChild>{link}</ContextMenuTrigger>
            <ContextMenuContent>
                <ContextMenuItem onSelect={() => void handleOfficeAction("preview")}>打开预览</ContextMenuItem>
                <ContextMenuItem onSelect={() => void handleOfficeAction("open_externally")}>
                    外部程序打开文件
                </ContextMenuItem>
                <ContextMenuItem onSelect={() => void handleOfficeAction("reveal")}>打开文件所在文件夹</ContextMenuItem>
            </ContextMenuContent>
        </ContextMenu>
    );
}
