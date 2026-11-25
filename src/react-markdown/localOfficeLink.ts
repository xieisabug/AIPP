import type { Root } from "hast";
import { visit } from "unist-util-visit";
import { isModernOfficeExtension } from "@/utils/officeFile";

export function parseLocalOfficeLink(href?: string) {
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
    if (/^[a-z][a-z\d+.-]*:/i.test(path) && !/^[a-z]:\//i.test(path)) return null;
    const title = path.slice(path.lastIndexOf("/") + 1);
    const extension = /\.([^.]+)$/.exec(title)?.[1].toLowerCase();
    return isModernOfficeExtension(extension) ? { path, title, extension } : null;
}

/** 在清理 HTML 之前把本地 Office 链接写成普通路径，沿用现有协议白名单。 */
export function rehypeLocalOfficeLinks() {
    return (tree: Root) => {
        visit(tree, "element", (node) => {
            if (node.tagName !== "a" || typeof node.properties.href !== "string") return;
            const link = parseLocalOfficeLink(node.properties.href);
            if (link) node.properties.href = link.path.split("/").map(encodeURIComponent).join("/");
        });
    };
}
