import type { ComponentProps } from "react";
import type { ExtraProps } from "react-markdown";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Braces, CodeXml, FileText, FolderGit2, Globe, Hash, Image, Mail, Workflow } from "lucide-react";
import { toast } from "sonner";
import { ContextMenu, ContextMenuContent, ContextMenuItem, ContextMenuTrigger } from "@/components/ui/context-menu";
import { parseLocalOfficeLink } from "@/react-markdown/localOfficeLink";
import { openOfficePreviewModal } from "@/utils/officeFile";

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

type MarkdownLinkProps = ComponentProps<"a"> & ExtraProps & { conversationId?: number; enableOfficeLinks?: boolean };

export function MarkdownLink({ node: _node, href, children, className, conversationId, enableOfficeLinks = true, ...props }: MarkdownLinkProps) {
    const office = enableOfficeLinks ? parseLocalOfficeLink(href) : null;
    const Icon = office ? FileText : href ? iconForLink(href) : null;
    const handleOfficeAction = async (action: "preview" | "open_externally" | "reveal") => {
        if (!office) return;
        try {
            const relayToken = await invoke<string | null>("handle_office_file_link", { path: office.path, conversationId, action });
            if (action === "preview") {
                if (!relayToken) throw new Error("未取得文件预览凭据");
                openOfficePreviewModal({ title: office.title, extension: office.extension, source: { relayToken }, conversationId });
            }
        } catch (error) {
            toast.error(String(error));
        }
    };
    const link = (
        <a {...props} href={href} className={`aipp-markdown-link ${className ?? ""}`}
            onClick={(event) => {
                event.preventDefault();
                if (office) void handleOfficeAction(event.ctrlKey || event.metaKey ? "open_externally" : "preview");
                else if (href) openUrl(href).catch(console.error);
            }}
        >
            {Icon && <Icon aria-hidden="true" className="me-[0.25em] inline-block size-[0.9em] align-[-0.1em]" />}
            {children}
        </a>
    );
    if (!office) return link;
    return (
        <ContextMenu>
            <ContextMenuTrigger asChild>{link}</ContextMenuTrigger>
            <ContextMenuContent>
                <ContextMenuItem onSelect={() => void handleOfficeAction("preview")}>打开预览</ContextMenuItem>
                <ContextMenuItem onSelect={() => void handleOfficeAction("open_externally")}>外部程序打开文件</ContextMenuItem>
                <ContextMenuItem onSelect={() => void handleOfficeAction("reveal")}>打开文件所在文件夹</ContextMenuItem>
            </ContextMenuContent>
        </ContextMenu>
    );
}
