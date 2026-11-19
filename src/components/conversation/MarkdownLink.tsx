import type { Components } from "react-markdown";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Braces, CodeXml, FileText, FolderGit2, Globe, Hash, Image, Mail, Workflow } from "lucide-react";

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

export const MarkdownLink: NonNullable<Components["a"]> = ({ node: _node, href, children, className, ...props }) => {
    const Icon = href ? iconForLink(href) : null;
    return (
        <a {...props} href={href} className={`aipp-markdown-link ${className ?? ""}`}
            onClick={(event) => {
                event.preventDefault();
                if (href) openUrl(href).catch(console.error);
            }}
        >
            {Icon && <Icon aria-hidden="true" className="me-[0.25em] inline-block size-[0.9em] align-[-0.1em]" />}
            {children}
        </a>
    );
};
