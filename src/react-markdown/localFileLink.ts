import type { Root } from "hast";
import { visit } from "unist-util-visit";
import { isModernOfficeExtension } from "@/utils/officeFile";
import { inferLanguageFromPath } from "@/utils/fileLanguage";

const mediaFormats = {
    image: ["png", "jpg", "jpeg", "gif", "webp", "avif", "bmp", "ico", "apng"],
    audio: ["mp3", "wav", "ogg", "oga", "flac", "m4a", "aac", "opus"],
    video: ["mp4", "webm", "m4v", "mov", "ogv"],
} as const;

export function parseLocalFileLink(href?: string) {
    if (!href) return null;
    let raw = href.split(/[?#]/, 1)[0];
    if (/^file:/i.test(raw)) {
        try {
            const url = new URL(raw);
            if (url.hostname && url.hostname !== "localhost") return null;
            raw = url.pathname;
        } catch {
            return null;
        }
    }
    let path: string;
    try {
        path = decodeURIComponent(raw).replace(/\\/g, "/");
    } catch {
        return null;
    }
    if (path.startsWith("//") || /[\u0000-\u001f]/.test(path)) return null;
    // file:///C:/... 在 URL 中有前导斜线，原生 Windows 路径没有。
    path = path.replace(/^\/([a-z]:\/)/i, "$1");
    // Agent 回复常用 path:line[:column] 引用源码。
    path = path.replace(/(\.[a-z\d]+):\d+(?::\d+)?$/i, "$1");
    if (/^[a-z][a-z\d+.-]*:/i.test(path) && !/^[a-z]:\//i.test(path)) return null;
    if (!path) return null;
    const title = path.replace(/\/$/, "").split("/").pop() || path;
    if (path.endsWith("/")) return { kind: "reveal" as const, path, title };
    const extension = /\.([^.]+)$/.exec(title)?.[1].toLowerCase();
    if (isModernOfficeExtension(extension)) return { kind: "office" as const, path, title, extension };
    if (extension === "csv" || extension === "tsv") return { kind: "table" as const, path, title, extension };
    if (extension === "pdf") return { kind: "pdf" as const, path, title };
    for (const kind of ["image", "audio", "video"] as const) {
        if ((mediaFormats[kind] as readonly string[]).includes(extension ?? "")) return { kind, path, title };
    }
    if (extension === "drawio" || title.toLowerCase().endsWith(".drawio.xml"))
        return { kind: "artifact" as const, language: "drawio", path, title };
    if (extension === "mmd" || extension === "mermaid")
        return { kind: "artifact" as const, language: "mermaid", path, title };
    if (extension === "svg") return { kind: "artifact" as const, language: "svg", path, title };
    const language = inferLanguageFromPath(title);
    if (language === "html") return { kind: "artifact" as const, path, title, language };
    if (language) return { kind: "text" as const, path, title, language };
    if (extension || path.includes("/")) return { kind: "reveal" as const, path, title };
    return null;
}

export type LocalFileLink = NonNullable<ReturnType<typeof parseLocalFileLink>>;

/** 在清理 HTML 之前把支持预览的本地链接写成普通路径，沿用现有协议白名单。 */
export function rehypeLocalFileLinks() {
    return (tree: Root) => {
        visit(tree, "element", (node) => {
            if (node.tagName !== "a" || typeof node.properties.href !== "string") return;
            const link = parseLocalFileLink(node.properties.href);
            if (link) node.properties.href = link.path.split("/").map(encodeURIComponent).join("/");
        });
    };
}
